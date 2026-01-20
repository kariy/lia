# Agent Sidecar

The agent-sidecar is a minimal Rust binary that runs inside each Firecracker microVM. It manages the Claude Code process and relays I/O between the process and the host VM API via vsock.

## Architecture

```
vm/agent-sidecar/
├── src/
│   └── main.rs      # Complete implementation
└── Cargo.toml       # Dependencies and build config
```

## Design Goals

- **Minimal footprint**: Optimized for size, statically linked with musl
- **Zero-copy I/O**: 4KB buffers for efficient streaming
- **Simple concurrency**: OS threads for deterministic behavior
- **Fast fail**: No retries (VMs are ephemeral)
- **Portability**: musl-linked binary works on any Linux

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

The sidecar runs Claude Code as a non-root user (`claude`) because `--dangerously-skip-permissions` cannot be used with root privileges for security reasons.

```rust
Command::new("sudo")
    .arg("-u").arg("claude")
    .arg("-E")  // Preserve environment (for ANTHROPIC_API_KEY)
    .arg("--")
    .arg("/home/claude/.local/bin/claude")
    .arg("--print")
    .arg("--input-format").arg("stream-json")
    .arg("--output-format").arg("stream-json")
    .arg("--verbose")
    .arg("--include-partial-messages")
    .arg("--dangerously-skip-permissions")
    .env("ANTHROPIC_API_KEY", &api_key)
    .env("HOME", "/home/claude")
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
- Working directory: `/workspace` (owned by claude user)
- API key via environment (never touches disk)
- All streams piped for relay
- **Persistent process**: Claude Code stays running, accepting multiple prompts via stdin
- Initial prompt sent via stdin as JSON (not as CLI argument)
- **Non-root execution**: Claude runs as `claude` user via sudo

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
- **Sidecar binary**: `/usr/local/bin/agent-sidecar`
- **Claude binary**: `/usr/local/share/claude/versions/<version>`
- **Claude symlink**: `/home/claude/.local/bin/claude`
- **Working directory**: `/workspace` (owned by claude user)

## Service Configuration

The sidecar runs as a systemd service on Debian:

```ini
# /etc/systemd/system/agent-sidecar.service
[Unit]
Description=Lia Agent Sidecar
After=network.target lia-network-init.service

[Service]
Type=simple
ExecStart=/usr/local/bin/agent-sidecar
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal
Environment="PATH=/home/claude/.local/bin:/usr/local/bin:/usr/bin:/bin"
WorkingDirectory=/workspace

[Install]
WantedBy=multi-user.target
```

## Development

```bash
# Build for development (host system)
cargo build

# Build for production (optimized, host system)
cargo build --release
```

## Building for VM (Recommended: musl static linking)

Always use musl static linking for the sidecar binary. This ensures portability across different Linux distributions and glibc versions.

```bash
# Install musl target (one-time)
rustup target add x86_64-unknown-linux-musl

# Install musl tools (Debian/Ubuntu)
sudo apt-get install musl-tools

# Build statically-linked binary
cargo build --release --target x86_64-unknown-linux-musl

# Output: target/x86_64-unknown-linux-musl/release/agent-sidecar
```

**Why musl?** The host system may have a newer glibc than the VM rootfs. For example, Ubuntu 24.04 has glibc 2.39 while Debian Bookworm has glibc 2.36. A glibc-linked binary built on the host will fail with:

```
/usr/local/bin/agent-sidecar: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found
```

musl produces a fully static binary that works everywhere.

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

## VM Rootfs Requirements

The Debian rootfs must include:

1. **claude user**: Non-root user for running Claude Code
2. **haveged**: Entropy daemon (VMs lack hardware entropy sources)
3. **sudo**: For sidecar to run Claude as claude user
4. **Claude Code**: Installed in shared location `/usr/local/share/claude/`

The build script (`vm/rootfs/build-rootfs.sh`) handles all of this automatically.

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

### "GLIBC_X.XX not found"

The sidecar binary was built with glibc but the rootfs has an older glibc version.

**Fix**: Rebuild with musl target:

```bash
cargo build --release --target x86_64-unknown-linux-musl
```

### "--dangerously-skip-permissions cannot be used with root"

Claude Code refuses to run with `--dangerously-skip-permissions` when executed as root.

**Fix**: The sidecar runs Claude as the `claude` user via sudo. Ensure:
1. The `claude` user exists in the rootfs
2. Sudo is configured in `/etc/sudoers.d/agent-sidecar`:
   ```
   root ALL=(claude) NOPASSWD: ALL
   ```
3. Claude binary is accessible at `/home/claude/.local/bin/claude`

### "getrandom indicates that the entropy pool has not been initialized"

VMs often lack sufficient entropy for cryptographic operations.

**Fix**: Install and enable haveged in the rootfs:

```bash
apt-get install -y haveged
systemctl enable haveged
```

### Sidecar keeps restarting (service restart loop)

Check the systemd journal for errors:

```bash
# Inside VM or mount rootfs
journalctl -u agent-sidecar.service
```

Common causes:
- vsock not available (kernel module issue)
- Claude binary not found or not executable
- Permission issues with /workspace

### Checking sidecar logs

The sidecar logs to systemd journal. To check from outside the VM:

```bash
# Mount the VM's rootfs (while VM is stopped)
sudo mount /var/lib/lia/volumes/{task-id}-rootfs.ext4 /mnt/vmrootfs

# Check journal
sudo journalctl --root=/mnt/vmrootfs -u agent-sidecar.service

sudo umount /mnt/vmrootfs
```

## Integration Testing

Run the Claude streaming integration tests:

```bash
cd services/vm-api
sudo ANTHROPIC_API_KEY=sk-... cargo test --test claude_streaming_test -- --nocapture --test-threads=1
```

### Available Tests

| Test | Purpose | Duration |
|------|---------|----------|
| `test_claude_streaming_via_vsock` | Single-turn streaming verification | ~2 min |
| `test_claude_multiturn_streaming` | Two-turn context retention | ~3 min |
| `test_claude_comprehensive_conversation` | Full end-to-end with 8 turns | ~10 min |

### Single-Turn Test (`test_claude_streaming_via_vsock`)

Basic test that:
1. Starts a Firecracker VM with vsock
2. Connects to the sidecar via vsock
3. Sends an Init message with a prompt
4. Verifies streaming output (system init, stream events, result)

### Multi-Turn Test (`test_claude_multiturn_streaming`)

Tests conversation context retention:
1. Turn 1: "Remember this number: 42"
2. Turn 2: "What number did I ask you to remember?"
3. Verifies Claude retained context

### Comprehensive Test (`test_claude_comprehensive_conversation`)

Full end-to-end test with 8 turns covering:

1. **File upload**: Sends 3 files (data.json, config.txt, src/main.py) with Init message
2. **File reading**: Claude reads and analyzes data.json
3. **File creation**: Claude creates output.txt using tools
4. **File verification**: Claude verifies the file was created
5. **Context retention**: Claude recalls info from earlier turns without re-reading files
6. **File modification**: Claude edits config.txt (changes debug=false to debug=true)
7. **Bash execution**: Claude runs `python3 src/main.py`
8. **Summary**: Claude summarizes all actions in the conversation

Run just the comprehensive test:

```bash
sudo ANTHROPIC_API_KEY=sk-... cargo test --test claude_streaming_test test_claude_comprehensive_conversation -- --nocapture --test-threads=1
```
