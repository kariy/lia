# Lia - Discord Bot AI Agent Platform

## Overview
A Discord bot that spawns AI agents (Claude Code) in isolated Firecracker microVMs, with a rich web UI for bidirectional interaction.

## Architecture

```
┌─────────────────┐      HTTP/REST      ┌─────────────────────────┐
│   Discord Bot   │ ──────────────────> │    VM Management API    │
│ (TypeScript/Bun)│ <────────────────── │       (Rust/Axum)       │
└─────────────────┘     Task ID + URL   └───────────┬─────────────┘
                                                    │ vsock
┌─────────────────┐      WebSocket      ┌───────────┴─────────────┐
│     Web UI      │ <────────────────── │   Firecracker microVMs  │
│  (React/Vite)   │ ──────────────────> │     (Claude Code)       │
└─────────────────┘   Bidirectional I/O └─────────────────────────┘
```

## Components

### 1. Discord Bot (`packages/discord-bot/`)
- **Stack**: TypeScript, Bun, discord.js
- **Commands**:
  - `/spawn <prompt>` - Create new AI agent
  - `/spawn-file <prompt> <attachment>` - Spawn with file context
  - `/status <task_id>` - Check task status
  - `/resume <task_id>` - Wake up a suspended VM
  - `/stop <task_id>` - Terminate agent and release resources
  - `/list` - List user's active/suspended agents

### 2. VM Management API (`services/vm-api/`)
- **Stack**: Rust, Axum, SQLx (PostgreSQL), tokio
- **Responsibilities**:
  - Firecracker VM lifecycle (create, start, stop, destroy)
  - Task state machine (pending → running → suspended → terminated)
  - WebSocket streaming to Web UI
  - vsock relay for host ↔ guest communication
  - API key injection via environment variable

### 3. Web UI (`packages/web-ui/`)
- **Stack**: React, Vite, xterm.js, Tailwind
- **Features**:
  - Real-time terminal output display
  - Bidirectional input (send follow-up prompts)
  - Task status and metadata display
  - Resume/Terminate controls

### 4. Agent Sidecar (`vm/agent-sidecar/`)
- **Stack**: Rust (minimal binary)
- **Runs inside VM**, manages Claude Code process and vsock I/O

### 5. VM Infrastructure (`vm/`)
- Minimal Alpine Linux rootfs with Claude Code pre-installed
- Custom kernel config for Firecracker
- vsock device for host communication
- TAP networking for Claude API access
- **50GB storage volume** per VM (sparse file, allocated on demand)

### 6. Lifecycle & Cleanup
- VMs persist until user explicitly terminates (not auto-terminated on agent exit)
- **Auto-suspend**: VMs automatically suspend after configurable idle timeout (default: 30 min)
- **Resume on demand**: Suspended VMs can be woken up via Web UI or `/resume` command
- User triggers full cleanup via Web UI "End Session" button or `/stop` command
- Cleanup releases:
  - Firecracker VM process
  - 50GB storage volume
  - vsock sockets
  - Task output buffers

### 7. VM States
```
                    ┌──────────────────────────────┐
                    │                              │
                    v                              │
pending → starting → running ──(idle timeout)──> suspended
                       │                            │
                       │                            │ (user resume)
                       │                            v
                       │                          running
                       │
                       └──(user terminate)──> terminated (cleanup)
```

## Project Structure

```
lia/
├── packages/
│   ├── discord-bot/        # Discord Bot (TypeScript/Bun)
│   ├── web-ui/             # Web UI (React/Vite)
│   └── shared/             # Shared TypeScript types
├── services/
│   └── vm-api/             # VM Management API (Rust)
├── vm/
│   ├── agent-sidecar/      # Sidecar binary (Rust)
│   ├── rootfs/             # Rootfs build scripts
│   └── kernel/             # Kernel config
├── infra/                  # Terraform/Ansible
└── docs/
```

## API Design

### REST Endpoints (VM API)
```
POST   /api/v1/tasks                  # Create new agent task
GET    /api/v1/tasks/{id}             # Get task details
POST   /api/v1/tasks/{id}/resume      # Resume a suspended VM
DELETE /api/v1/tasks/{id}             # Terminate and cleanup
GET    /api/v1/tasks/{id}/output      # Get buffered output
WS     /api/v1/tasks/{id}/stream      # Bidirectional WebSocket
```

### Task States
- `pending` - Waiting for VM allocation
- `starting` - VM booting
- `running` - Agent active, accepting input
- `suspended` - VM paused after idle timeout (storage preserved)
- `terminated` - User ended session, all resources released

Note: Suspended VMs preserve 50GB storage and can be resumed. Only `terminated` triggers full cleanup.

## Data Flow

### Spawn Flow
1. User runs `/spawn "help me build X"` in Discord
2. Discord Bot calls `POST /api/v1/tasks` on VM API
3. VM API creates task record, allocates Firecracker VM with 50GB sparse volume
4. VM boots, agent sidecar connects via vsock
5. API injects Claude API key + prompt via vsock
6. Discord Bot replies with task link: `https://lia.example.com/tasks/{id}`
7. User opens link, Web UI connects via WebSocket
8. Output streams in real-time, user can send follow-up prompts

### Suspend Flow (Automatic)
1. VM idle timeout reached (configurable, default 30 min)
2. VM API calls Firecracker Pause API
3. VM enters suspended state (CPU halted, memory frozen)
4. Storage volume preserved on disk
5. User notified via Discord DM (optional)

### Resume Flow
1. User clicks "Resume" in Web UI or runs `/resume {task_id}` in Discord
2. VM API calls Firecracker Resume API
3. VM continues from where it left off
4. WebSocket reconnects, output streaming resumes

### Terminate Flow (User-Initiated)
1. User clicks "End Session" in Web UI or runs `/stop {task_id}` in Discord
2. VM API sends SIGTERM to Firecracker process
3. VM shuts down gracefully
4. VM API deletes:
   - 50GB storage volume file (`/var/lib/lia/volumes/{task_id}.ext4`)
   - vsock socket file
   - Output buffer from memory/Redis
5. Task marked as `terminated` in database (metadata retained for history)

## Security

- **Isolation**: Firecracker microVMs with KVM hardware virtualization
- **Jailer**: chroot, seccomp, cgroups, dropped privileges
- **Network**: Per-VM namespace, egress-only, API endpoint allowlist
- **API Key**: Memory-only injection, never persisted to disk
- **Auth**: Discord OAuth2 + task-specific tokens for Web UI

## Database Schema

```sql
CREATE TABLE tasks (
    id UUID PRIMARY KEY,
    user_id VARCHAR(64) NOT NULL,
    guild_id VARCHAR(64),
    prompt TEXT NOT NULL,
    status VARCHAR(32) NOT NULL,
    vm_id VARCHAR(64),
    config JSONB,
    created_at TIMESTAMPTZ,
    started_at TIMESTAMPTZ,
    suspended_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    exit_code INTEGER,
    error_message TEXT
);
```

## Implementation Phases

### Phase 1: Core Infrastructure
- [ ] Set up Firecracker on dev machine
- [ ] Build minimal rootfs (Alpine + Claude Code)
- [ ] Implement agent sidecar with vsock
- [ ] Create VM API skeleton with task CRUD

### Phase 2: VM Lifecycle
- [ ] Implement VM spawn/terminate
- [ ] Add vsock relay for I/O streaming
- [ ] Implement task state machine
- [ ] Add PostgreSQL persistence
- [ ] Implement suspend/resume functionality

### Phase 3: Discord Bot
- [ ] Create Discord application
- [ ] Implement slash commands
- [ ] Integrate with VM API
- [ ] Add embeds and status updates

### Phase 4: Web UI
- [ ] Build React + Vite application
- [ ] Implement terminal with xterm.js
- [ ] Add WebSocket streaming
- [ ] Implement bidirectional input
- [ ] Add Resume/Terminate controls

### Phase 5: Security & Production
- [ ] Configure Jailer
- [ ] Implement rate limiting
- [ ] Add secrets management
- [ ] Security audit

## Verification

1. **Unit Tests**: Each component has unit tests
2. **Integration Tests**:
   - Discord Bot ↔ VM API communication
   - VM API ↔ Firecracker lifecycle
   - Web UI ↔ WebSocket streaming
3. **E2E Test**:
   - Run `/spawn "create a hello world python script"` in Discord
   - Open returned link in browser
   - Verify output streams in real-time
   - Send follow-up prompt "now add error handling"
   - Verify agent responds and updates code
   - Wait for auto-suspend, then `/resume` and verify continuation
   - Run `/stop` and verify cleanup

## Key Dependencies

- **Discord Bot**: discord.js, zod
- **VM API**: axum, tokio, sqlx, firepilot (Firecracker SDK)
- **Web UI**: react, vite, @xterm/xterm, zustand, react-router-dom
- **Sidecar**: tokio, nix (for PTY/vsock)

## Hosting

- **Target**: Self-hosted bare metal server with KVM support
- **Requirements**: Linux kernel with KVM enabled, Firecracker binary
- **Development**: Firecracker directly (no Docker abstraction layer)
