# Lia - Discord Bot AI Agent Platform

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
└── docs/
```

## Prerequisites

### System Requirements

- Linux server with KVM support (check with `ls /dev/kvm`)
- x86_64 architecture
- At least 8GB RAM (more recommended for running multiple VMs)
- Root access for Firecracker setup

### System Dependencies (Ubuntu/Debian)

```bash
# Update package lists
sudo apt update

# Install build essentials
sudo apt install -y build-essential pkg-config libssl-dev

# Install networking tools (required for VM networking)
sudo apt install -y iptables iproute2 bridge-utils

# Install utilities
sudo apt install -y curl wget git jq

# Verify KVM support
sudo apt install -y cpu-checker
kvm-ok
```

### Rust (1.75+)

```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Restart shell or source cargo env
source "$HOME/.cargo/env"

# Verify installation
rustc --version
cargo --version
```

### Bun (1.0+)

```bash
# Install bun
curl -fsSL https://bun.sh/install | bash

# Restart shell or add to PATH
export PATH="$HOME/.bun/bin:$PATH"

# Verify installation
bun --version
```

### PostgreSQL (15+)

```bash
# Install PostgreSQL
sudo apt install -y postgresql postgresql-contrib

# Start and enable PostgreSQL
sudo systemctl start postgresql
sudo systemctl enable postgresql

# Create database and user
sudo -u postgres createuser --createdb $USER
createdb lia

# Verify connection
psql -d lia -c "SELECT version();"
```

### mprocs (for development)

```bash
# Install mprocs (TUI for running multiple processes)
cargo install mprocs

# Verify installation
mprocs --version
```

### Optional: Node.js (for some tooling)

```bash
# If needed for certain build tools
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt install -y nodejs
```

## Quick Start

### 1. Setup Infrastructure

```bash
# Install Firecracker and create directories
sudo bash vm/setup.sh

# Build the rootfs (requires root)
cd vm/rootfs
sudo bash build-rootfs.sh
```

### 2. Build Components

```bash
# Build agent sidecar
cd vm/agent-sidecar
cargo build --release
sudo cp target/release/agent-sidecar /var/lib/lia/rootfs/

# Build VM API
cd services/vm-api
cargo build --release

# Install TypeScript dependencies
bun install
```

### 3. Configure Environment

```bash
cp .env.example .env
# Edit .env with your configuration
```

### 4. Setup Database

```bash
# Create PostgreSQL database
createdb lia
# Migrations run automatically on API start
```

### 5. Start Services

```bash
# Terminal 1: VM API
cd services/vm-api
cargo run --release

# Terminal 2: Discord Bot
cd packages/discord-bot
bun run dev

# Terminal 3: Web UI
cd packages/web-ui
bun run dev
```

### 6. Deploy Discord Commands

```bash
cd packages/discord-bot
bun run deploy-commands
```

## Discord Commands

| Command | Description |
|---------|-------------|
| `/spawn <prompt>` | Create new AI agent |
| `/spawn-file <prompt> <file>` | Spawn with file context |
| `/status <task_id>` | Check task status |
| `/resume <task_id>` | Wake up a suspended VM |
| `/stop <task_id>` | Terminate agent and release resources |
| `/list` | List user's active/suspended agents |

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/v1/tasks` | Create new agent task |
| GET | `/api/v1/tasks/{id}` | Get task details |
| POST | `/api/v1/tasks/{id}/resume` | Resume a suspended VM |
| DELETE | `/api/v1/tasks/{id}` | Terminate and cleanup |
| GET | `/api/v1/tasks/{id}/output` | Get buffered output |
| WS | `/api/v1/tasks/{id}/stream` | Bidirectional WebSocket |

## Task States

```
pending → starting → running ──(idle timeout)──> suspended
                       │                            │
                       │                            │ (user resume)
                       │                            v
                       │                          running
                       │
                       └──(user terminate)──> terminated (cleanup)
```

## Configuration

Configuration can be set via environment variables or config files:

| Variable | Description | Default |
|----------|-------------|---------|
| `LIA__SERVER__PORT` | API server port | 3000 |
| `LIA__DATABASE__URL` | PostgreSQL connection URL | - |
| `LIA__CLAUDE__API_KEY` | Anthropic API key | - |
| `LIA__VM__IDLE_TIMEOUT_MINUTES` | Auto-suspend timeout | 30 |
| `LIA__VM__DEFAULT_STORAGE_GB` | VM storage size | 50 |

See `.env.example` for all options.

## Security

- **Isolation**: Firecracker microVMs with KVM hardware virtualization
- **Jailer**: chroot, seccomp, cgroups, dropped privileges
- **Network**: Per-VM namespace, egress-only, API endpoint allowlist
- **API Key**: Memory-only injection, never persisted to disk

## Development

```bash
# Run all TypeScript typechecks
bun run typecheck

# Run VM API tests
cd services/vm-api
cargo test

# Run Discord bot in watch mode
cd packages/discord-bot
bun run dev
```

## License

MIT
