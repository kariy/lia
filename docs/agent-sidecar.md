# Agent Sidecar

The agent-sidecar is a minimal Rust binary that runs inside each Firecracker microVM. It manages the Claude Code process and relays I/O between the process and the host VM API via vsock.

## Architecture

```
vm/agent-sidecar/
├── src/
│   └── main.rs      # Complete implementation (~329 lines)
└── Cargo.toml       # Dependencies and build config
```

## Design Goals

- **Minimal footprint**: Optimized for size in Alpine rootfs
- **Zero-copy I/O**: 4KB buffers for efficient streaming
- **Simple concurrency**: OS threads for deterministic behavior
- **Fast fail**: No retries (VMs are ephemeral)

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| tokio | 1.49.0 | Async runtime (for future use) |
| serde | 1.0.228 | Serialization |
| serde_json | 1.0.149 | JSON handling |
| nix | 0.28.0 | POSIX syscalls |
| libc | 0.2.180 | C library bindings |
| anyhow | 1.0.100 | Error handling |
| tracing | 0.1.44 | Structured logging |

## Build Optimization

```toml
[profile.release]
opt-level = "s"  # Size optimization
lto = true       # Link-time optimization
strip = true     # Strip debug symbols
```

## Communication Protocol

### vsock Connection

- **Port**: 5000 (hardcoded for sidecar)
- **Socket type**: Stream (SOCK_STREAM)
- **Connection model**: Listen/Accept (host-initiated)

The sidecar **listens** on vsock port 5000 and waits for the host to connect. This is the host-initiated connection model required by Firecracker's vsock multiplexer.

```
Host (VM API)                    Guest (Sidecar)
     |                                |
     |  CONNECT 5000\n via UDS        |
     |------------------------------->|
     |                                | (listening on port 5000)
     |<-------------------------------|
     |  OK <port>\n                   |
     |                                |
     |  Init message (JSON)           |
     |------------------------------->|
     |                                |
```

### Message Types

```rust
pub enum VsockMessage {
    Init { api_key: String, prompt: String, files: Option<Vec<TaskFile>> },
    Output { data: String },
    Input { data: String },
    Exit { code: i32 },
    Heartbeat,
}
```

Format: JSON Lines (`<json object>\n`)

## Process Flow

### 1. Initialization

```
1. Initialize tracing/logging
2. Listen on vsock port 5000 (bind + listen)
3. Accept connection from host
4. Read Init message from host
5. Extract: api_key, prompt, files
```

### 2. File Preparation

If files provided in Init message:
```
for each file:
    create parent directories under /workspace
    write file content
```

### 3. Claude Code Spawn

```rust
Command::new("claude")
    .arg("--print")
    .arg("--input-format").arg("stream-json")
    .arg("--output-format").arg("stream-json")
    .arg("--verbose")
    .arg("--include-partial-messages")
    .arg("--dangerously-skip-permissions")
    .env("ANTHROPIC_API_KEY", &api_key)
    .current_dir("/workspace")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
```

Key flags:

| Flag | Purpose |
|------|---------|
| `--print` | Non-interactive mode (no TUI) |
| `--input-format stream-json` | Accept JSON messages on stdin |
| `--output-format stream-json` | Emit JSON events on stdout |
| `--verbose` | Required for stream-json output |
| `--include-partial-messages` | Stream incremental token deltas |
| `--dangerously-skip-permissions` | Auto-approve tool calls (sandboxed VM) |

Key points:
- Working directory: `/workspace`
- API key via environment (never touches disk)
- All streams piped for relay
- **Persistent process**: Claude Code stays running, accepting multiple prompts via stdin
- Initial prompt sent via stdin as JSON (not as CLI argument)

See [claude-cli.md](./claude-cli.md) for complete documentation on programmatic usage.

### 4. Initial Prompt

After spawning Claude Code, the sidecar sends the initial prompt via stdin as JSON:

```json
{"type":"user","message":{"role":"user","content":"<prompt>"}}
```

This format allows follow-up messages to be sent the same way, enabling multi-turn conversations.

### 5. Three-Thread I/O Relay

**Thread 1: stdout → vsock (line-based)**
```
while running:
    read line from stdout (JSON event from Claude)
    wrap in VsockMessage::Output { data: line }
    send JSON line to vsock
    flush
```

Claude emits various JSON event types:
- `{"type":"system","subtype":"init",...}` - Session initialization
- `{"type":"stream_event","event":{"type":"content_block_delta",...}}` - Token deltas
- `{"type":"assistant","message":{...}}` - Complete message
- `{"type":"result",...}` - Final result

**Thread 2: stderr → vsock**
```
while running:
    read chunk from stderr (4KB buffer)
    wrap in Output message
    send JSON line to vsock
    flush
```

**Thread 3: vsock → stdin (convert to Claude JSON)**
```
while running:
    read JSON line from vsock
    parse VsockMessage
    if Input { data }:
        wrap in Claude format: {"type":"user","message":{"role":"user","content":"<data>"}}
        write JSON line to Claude's stdin, flush
    if Heartbeat: ignore (keep-alive)
```

This thread enables **multi-turn conversations**: users can send follow-up messages via the web UI, which are relayed to Claude Code as new user messages in the same session.

### 6. Shutdown

```
1. Wait for Claude Code to exit
2. Get exit code
3. Set running flag to false (atomic)
4. Send Exit message to host
5. Join all threads
6. Return
```

## Concurrency Model

- `Arc<AtomicBool>` shared flag for graceful shutdown
- `Ordering::Relaxed` for performance (no barrier needed)
- Each thread checks flag in loop condition
- Main thread sets flag after process exit

## Error Handling

- **Initialization errors**: Fatal, propagate up via `?`
- **Socket errors**: Explicit error construction with `anyhow::bail!`
- **Thread I/O errors**: Silent break (connection lost)
- **No retries**: Failures are fatal (VM will be destroyed)

## Host Integration

### Connection from VM API

The VM API (`services/vm-api/src/vsock.rs`) connects to the sidecar:

1. Retry connection up to 100 times (10s total)
2. Send Init message with API key, prompt, files
3. Spawn reader task for Output/Exit messages
4. Spawn writer task for Input messages

### Message Flow

```
Discord User → Discord Bot → VM API → vsock → Sidecar → Claude Code
                                                  ↓
Web UI ← WebSocket ← VM API ← vsock ← Sidecar ← stdout/stderr
```

## File Locations

In the VM:
- **Binary**: `/usr/local/bin/agent-sidecar`
- **Working directory**: `/workspace`
- **Log**: `/var/log/agent-sidecar.log`
- **PID file**: `/var/run/agent-sidecar.pid`

## Service Configuration

The sidecar runs as an OpenRC service:

```
/etc/init.d/agent-sidecar
├── Depends on: network
├── Runs as: background daemon
├── Logs to: /var/log/agent-sidecar.log
└── PID file: /var/run/agent-sidecar.pid
```

## Development

```bash
# Build for development (host system)
cargo build

# Build for production (optimized, host system)
cargo build --release
```

## Building for Alpine Linux (VM rootfs)

The VM rootfs uses Alpine Linux which uses **musl libc**, not glibc. You must cross-compile for the musl target:

```bash
# Install musl target (one-time)
rustup target add x86_64-unknown-linux-musl

# Install musl tools (Debian/Ubuntu)
sudo apt-get install musl-tools

# Build for Alpine
cargo build --release --target x86_64-unknown-linux-musl

# Output: target/x86_64-unknown-linux-musl/release/agent-sidecar
# Copy to rootfs: /usr/local/bin/agent-sidecar
```

> **Important**: A glibc-linked binary will fail to run on Alpine with "not found" errors (missing dynamic linker). Always use the musl target for the VM rootfs.

### Updating the rootfs

```bash
# Mount the rootfs template
sudo mount /var/lib/lia/rootfs/rootfs.ext4 /mnt/rootfs

# Copy the new binary
sudo cp target/x86_64-unknown-linux-musl/release/agent-sidecar /mnt/rootfs/usr/local/bin/

# Unmount
sudo umount /mnt/rootfs
```

New VMs will use the updated binary (each VM copies the rootfs template on creation).

## Troubleshooting

### "Bad file descriptor" on accept

If you see `Failed to accept vsock connection: Bad file descriptor (os error 9)`, the socket file descriptor is being closed prematurely. This happens when using `fd.as_raw_fd()` to return the fd from `listen_vsock()` - the `OwnedFd` gets dropped and closes the socket.

**Fix**: Use `fd.into_raw_fd()` to transfer ownership without closing:

```rust
// Wrong - fd is closed when OwnedFd is dropped
Ok(fd.as_raw_fd())

// Correct - ownership transferred, fd stays open
Ok(fd.into_raw_fd())
```

### Binary not running on Alpine

If the sidecar fails to start with no output or "not found" errors, check the binary type:

```bash
file agent-sidecar
```

- **Wrong**: `dynamically linked, interpreter /lib64/ld-linux-x86-64.so.2` (glibc)
- **Correct**: `static-pie linked` (musl)

Rebuild with `--target x86_64-unknown-linux-musl`.

### Checking sidecar logs

The sidecar logs to `/var/log/agent-sidecar.log` inside the VM. To check:

```bash
# Mount the VM's rootfs (while VM is stopped)
sudo mount /var/lib/lia/volumes/{task-id}-rootfs.ext4 /mnt/vmrootfs
sudo cat /mnt/vmrootfs/var/log/agent-sidecar.log
sudo umount /mnt/vmrootfs
```
