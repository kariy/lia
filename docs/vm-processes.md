# VM Process Architecture

This document describes the processes running inside each Firecracker microVM and how they interact. The key design principle is that **Claude Code runs as a persistent process** whose lifetime is bounded by the VM lifetime, and **SSH access is independent** of the Claude Code session.

## Process Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         FIRECRACKER VM                                   │
│                                                                          │
│  ┌─────────────────┐                                                    │
│  │   /sbin/init    │  (BusyBox init)                                    │
│  │   (PID 1)       │                                                    │
│  └────────┬────────┘                                                    │
│           │                                                              │
│           ├──────────────────┬──────────────────┐                       │
│           │                  │                  │                       │
│           ▼                  ▼                  ▼                       │
│  ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐           │
│  │  agent-sidecar  │ │     sshd        │ │     getty       │           │
│  │                 │ │   (port 22)     │ │   (ttyS0)       │           │
│  └────────┬────────┘ └─────────────────┘ └─────────────────┘           │
│           │                  │                                          │
│           ▼                  ▼                                          │
│  ┌─────────────────┐ ┌─────────────────┐                               │
│  │  claude         │ │  user shells    │                               │
│  │  (Claude Code)  │ │  (via SSH)      │                               │
│  └─────────────────┘ └─────────────────┘                               │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## Process Details

### 1. Init System (PID 1)

- **Binary**: `/sbin/init` → BusyBox init
- **Config**: `/etc/inittab`
- **Role**: Starts OpenRC, spawns services, manages process lifecycle

```
# /etc/inittab
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default
ttyS0::respawn:/sbin/getty -L ttyS0 115200 vt100
::shutdown:/sbin/openrc shutdown
```

### 2. Agent Sidecar

- **Binary**: `/usr/local/bin/agent-sidecar`
- **Started by**: OpenRC (`/etc/init.d/agent-sidecar`)
- **Depends on**: Network
- **Role**: Manages Claude Code process, relays I/O via vsock

**Lifecycle**:
1. Starts on VM boot (OpenRC default runlevel)
2. Listens on vsock port 5000
3. Accepts connection from host VM API
4. Receives Init message (API key, prompt, files)
5. Spawns Claude Code process
6. Relays I/O between Claude Code and host
7. Exits when Claude Code exits

### 3. Claude Code

- **Binary**: `/usr/local/bin/claude` (npm-installed)
- **Started by**: agent-sidecar
- **Working directory**: `/workspace`
- **Role**: AI coding assistant, executes tasks

**Invocation**:
```bash
claude --print \
       --input-format stream-json \
       --output-format stream-json \
       --verbose \
       --dangerously-skip-permissions \
       -p "<initial-prompt>"
```

**Key characteristics**:
- **Persistent session**: Runs until task completion or VM termination
- **Multi-turn capable**: Accepts follow-up messages via stdin (stream-json)
- **Non-interactive**: No TUI, pure JSON I/O
- **Sandboxed**: All tool calls auto-approved (`--dangerously-skip-permissions`)

### 4. SSH Daemon

- **Binary**: `/usr/sbin/sshd`
- **Started by**: OpenRC (`/etc/init.d/sshd`)
- **Port**: 22
- **Role**: Allows direct user access to VM

**Configuration** (`/etc/ssh/sshd_config`):
- Root login: Yes (pub key only)
- Password auth: No
- Key file: `/root/.ssh/authorized_keys`

### 5. Getty (Serial Console)

- **Binary**: `/sbin/getty`
- **Started by**: init (respawn)
- **Device**: ttyS0 at 115200 baud
- **Role**: Emergency console access

## Process Independence

```
                    ┌─────────────────────────────────────────┐
                    │           INDEPENDENT PROCESSES         │
                    │                                         │
  vsock ◄──────────►│  agent-sidecar ◄────► claude           │
  (to host)         │       │                  │              │
                    │       │                  ▼              │
                    │       │            /workspace           │
                    │       │                  ▲              │
  SSH ◄────────────►│     sshd ◄────────► user shell ────────┤
  (port 22)         │                                         │
                    └─────────────────────────────────────────┘
```

**Key points**:

1. **SSH doesn't interrupt Claude Code**: SSH sessions run in separate process trees
2. **Shared workspace**: Both Claude Code and SSH users access `/workspace`
3. **Concurrent access**: Multiple SSH sessions + Claude Code can run simultaneously
4. **Independent I/O**: Claude Code streams to vsock; SSH uses TCP port 22

## Data Flow

### Claude Code Output (uninterrupted streaming)

```
Claude Code stdout
       │
       ▼ (JSON lines)
agent-sidecar (stdout reader thread)
       │
       ▼ (VsockMessage::Output)
vsock port 5000
       │
       ▼
Host VM API (VsockRelay)
       │
       ▼ (WsMessage::Output)
WebSocket
       │
       ▼
Web UI
```

### User SSH Session (independent)

```
User's terminal
       │
       ▼ (SSH protocol)
TCP port 22 ──► lia-br0 bridge ──► VM eth0
       │
       ▼
sshd
       │
       ▼
User's shell (ash/bash)
       │
       ▼
/workspace (shared with Claude Code)
```

### User Input to Claude Code

```
Web UI
       │
       ▼ (WsMessage::Input)
WebSocket
       │
       ▼
Host VM API (VsockRelay)
       │
       ▼ (VsockMessage::Input)
vsock port 5000
       │
       ▼
agent-sidecar (input thread)
       │
       ▼ (JSON: {"type":"user","message":{...}})
Claude Code stdin
```

## Process Lifecycle

### VM Boot Sequence

```
1. Firecracker starts VM
2. Kernel boots, runs /sbin/init
3. init runs OpenRC sysinit
4. init runs OpenRC boot
   └─► lia-init: configures network, SSH keys
5. init runs OpenRC default
   ├─► networking: brings up eth0
   ├─► sshd: starts SSH daemon
   └─► agent-sidecar: starts sidecar service
6. getty spawns on ttyS0 (console)
```

### Agent Sidecar Lifecycle

```
1. OpenRC starts agent-sidecar service
2. Sidecar binds to vsock port 5000
3. Sidecar waits for host connection (blocking)
4. Host connects, sends Init message
5. Sidecar spawns Claude Code with prompt
6. Three relay threads start:
   - stdout → vsock
   - stderr → vsock
   - vsock → stdin
7. Claude Code executes task
8. Claude Code exits (task complete or error)
9. Sidecar sends Exit message to host
10. Sidecar process exits
11. VM can be terminated or suspended
```

### SSH Session Lifecycle (Independent)

```
1. User connects: ssh root@172.16.0.X
2. sshd authenticates via public key
3. sshd spawns user shell
4. User can:
   - View /workspace (Claude's working directory)
   - Run commands
   - Edit files
   - Monitor Claude's progress
5. User exits shell
6. SSH session closes
7. Claude Code continues unaffected
```

## File System Layout

```
/
├── workspace/              # Shared working directory
│   ├── (user files)        # Files from Init message
│   └── (Claude's work)     # Files created by Claude Code
├── var/
│   ├── log/
│   │   └── agent-sidecar.log
│   └── run/
│       └── agent-sidecar.pid
├── root/
│   └── .ssh/
│       └── authorized_keys  # Set by lia-init from boot params
└── etc/
    └── init.d/
        ├── lia-init         # Network/SSH setup
        ├── agent-sidecar    # Sidecar service
        ├── sshd             # SSH daemon
        └── networking       # Network config
```

## Environment Variables

### Claude Code Process

| Variable | Value | Source |
|----------|-------|--------|
| `ANTHROPIC_API_KEY` | API key | Init message via sidecar |
| `HOME` | `/root` | Default |
| `PWD` | `/workspace` | Set by sidecar |
| `PATH` | Standard Alpine PATH | Default |

### SSH Sessions

| Variable | Value | Source |
|----------|-------|--------|
| `HOME` | `/root` | sshd |
| `USER` | `root` | sshd |
| `SHELL` | `/bin/ash` | passwd |
| `SSH_CONNECTION` | Client info | sshd |

## Monitoring Processes

### From inside VM (via SSH)

```bash
# List all processes
ps aux

# Watch Claude Code
ps aux | grep claude

# Check sidecar logs
tail -f /var/log/agent-sidecar.log

# Check workspace
ls -la /workspace
```

### From host

```bash
# Check Firecracker process
ps aux | grep firecracker

# Check VM API logs
journalctl -u lia-vm-api -f

# SSH into VM
ssh root@172.16.0.X
```

## Signals and Termination

| Signal | Effect on Claude Code | Effect on Sidecar |
|--------|----------------------|-------------------|
| SIGTERM | Graceful shutdown | Graceful shutdown |
| SIGKILL | Immediate termination | Immediate termination |
| VM shutdown | Process killed | Process killed |

When the VM is terminated:
1. Firecracker sends SIGTERM to init
2. init signals all processes
3. Processes have brief grace period
4. Remaining processes killed
5. VM destroyed

## Security Considerations

1. **Claude Code sandboxing**: `--dangerously-skip-permissions` is safe because:
   - VM has no access to host filesystem
   - Network is isolated to VM subnet
   - No sensitive data on VM by default

2. **SSH access**: Public key authentication only
   - Keys injected via kernel boot parameters
   - No password authentication
   - Root access required for full workspace access

3. **Process isolation**: Each VM has:
   - Separate process namespace
   - Separate network namespace
   - Separate filesystem
   - No shared memory with host
