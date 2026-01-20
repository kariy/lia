# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Lia is a Discord bot platform that spawns Claude Code AI agents in isolated Firecracker microVMs. Users interact via Discord commands and a React web UI with real-time terminal streaming.

## Build Commands

```bash
# Install dependencies
bun install

# Build all components
make build

# Type checking (TypeScript + Rust)
make typecheck

# Run tests
make test

# Run SSH integration test (requires root + infrastructure)
make test-ssh
```

## Development

```bash
# Terminal 1: VM API (Rust)
make dev-api

# Terminal 2: Discord Bot
make dev-bot

# Terminal 3: Web UI (Vite dev server)
make dev-web

# Deploy Discord slash commands (one-time after changes)
make deploy-commands

# Run database migrations
make db-migrate
```

## Architecture

```
Discord User → Discord Bot (TypeScript/Bun) → VM API (Rust/Axum) → Firecracker VM
                                                   ↓
                                              PostgreSQL
                                                   ↓
Web UI (React/Vite) ←──WebSocket──→ VM API ←──vsock──→ Agent Sidecar → Claude Code
```

**Components:**
- `packages/discord-bot/` - Discord.js slash commands, calls VM API
- `packages/web-ui/` - React + xterm.js terminal, Zustand state, WebSocket streaming
- `packages/shared/` - Zod schemas shared between TypeScript packages
- `services/vm-api/` - Rust API server, Firecracker lifecycle, vsock relay, task persistence
- `vm/agent-sidecar/` - Rust binary inside VM, manages Claude Code process and vsock I/O
- `vm/rootfs/` - Alpine Linux rootfs build scripts
- `vm/kernel/` - Kernel download scripts

## Key Patterns

**Monorepo:** Bun workspaces for TypeScript packages. Shared types in `@lia/shared` with Zod validators.

**Task State Machine:** `pending → starting → running → suspended/terminated`. Suspended VMs preserve storage and can be resumed.

**Configuration (Rust):** Environment variables with `LIA__` prefix, double underscore separator (e.g., `LIA__SERVER__PORT`). Falls back to `config/local.toml` then `config/default.toml`.

**Database:** SQLx with compile-time query verification. Migrations in `services/vm-api/migrations/`.

**Communication Protocols:**
- REST: Task CRUD operations
- WebSocket: Real-time terminal I/O (`/api/v1/tasks/{id}/stream`)
- vsock: JSON-line protocol between host and VM sidecar

## Infrastructure Setup (requires root + KVM)

### Quick Reference

| Command | When to Use |
|---------|-------------|
| `make setup-all` | First-time setup on a new machine |
| `make setup` | Re-apply network config after reboot (if systemd service not enabled) |
| `sudo bash vm/rootfs/build-rootfs.sh` | Rebuild rootfs after modifying VM environment |

### Scripts Overview

#### `vm/setup-all.sh` - Complete Setup (First-Time)

**Use when:** Setting up a new development/production machine from scratch.

**What it does:**
1. Installs system dependencies (curl, iptables, bridge-utils, etc.)
2. Installs apk-tools for building Alpine rootfs
3. Creates `/var/lib/lia/` directory structure
4. Downloads and installs Firecracker v1.6.0
5. Downloads the Firecracker kernel
6. Builds the agent-sidecar binary
7. Builds the Alpine rootfs with Claude Code pre-installed
8. Sets up network bridge (`lia-br0`) and NAT rules
9. Creates systemd services for persistence

```bash
# Run complete setup
make setup-all
# or
sudo bash vm/setup-all.sh
```

#### `vm/setup.sh` - Quick Setup (Network Only)

**Use when:**
- Network bridge was deleted and needs recreation
- After system reboot if systemd service isn't working
- Re-applying iptables NAT rules

**What it does:**
1. Downloads Firecracker (if not present)
2. Downloads kernel (if not present)
3. Creates network bridge and NAT rules
4. Creates helper scripts (`lia-create-tap`, `lia-delete-tap`)
5. Creates systemd services

**Does NOT:** Build rootfs or agent-sidecar.

```bash
make setup
# or
sudo bash vm/setup.sh
```

#### `vm/rootfs/build-rootfs.sh` - Build VM Filesystem

**Use when:**
- First-time setup (called by `setup-all.sh`)
- After modifying packages installed in the VM
- After updating Claude Code version
- After modifying the agent-sidecar binary

**What it does:**
1. Creates 2GB sparse ext4 filesystem
2. Installs Alpine Linux 3.19 with OpenRC
3. Installs: nodejs, npm, git, openssh-server, python3, build tools
4. Installs Claude Code via npm
5. Copies agent-sidecar binary (if built)
6. Configures SSH (public key auth only)
7. Creates init scripts for networking and sidecar service

```bash
# Build the agent-sidecar first
cd vm/agent-sidecar && cargo build --release

# Then build rootfs
cd vm/rootfs && sudo bash build-rootfs.sh

# Copy to final location (setup-all.sh does this automatically)
sudo cp rootfs.ext4 /var/lib/lia/rootfs/
```

#### `vm/kernel/download-kernel.sh` - Download Kernel

**Use when:** Kernel file is missing or corrupted (rarely needed manually).

**What it does:** Downloads pre-built Firecracker kernel from AWS S3.

```bash
cd /var/lib/lia/kernel && sudo bash /path/to/vm/kernel/download-kernel.sh
```

### Directory Structure After Setup

```
/var/lib/lia/
├── kernel/vmlinux       # Firecracker kernel (~25MB)
├── rootfs/rootfs.ext4   # Alpine rootfs template (~500MB)
├── volumes/             # Per-VM persistent storage (created at runtime)
├── sockets/             # Firecracker API sockets (created at runtime)
├── logs/                # VM logs (created at runtime)
└── taps/                # TAP device info (created at runtime)
```

### Network Configuration

After setup, the network looks like:

- **Bridge:** `lia-br0` at 172.16.0.1/24
- **VM IPs:** 172.16.0.100-254 (assigned dynamically)
- **NAT:** VMs can access internet via host's primary interface

### Systemd Services

| Service | Purpose |
|---------|---------|
| `lia-network.service` | Recreates bridge and NAT rules on boot |
| `lia-vm-api.service` | Runs the VM API (for production) |

```bash
# Enable network persistence
sudo systemctl enable lia-network.service

# Check bridge status
ip addr show lia-br0
```

## Documentation

All documentation lives in `./docs`. When adding or modifying features:
- **Before making any changes** to a service, component, or feature, first read its corresponding documentation in `docs/` to understand the current implementation and intended behavior
- Create or update the corresponding documentation file for the affected component
- Each major component should have its own dedicated doc file (e.g., `docs/discord-bot.md`, `docs/vm-api.md`, `docs/web-ui.md`)
- Document the expected behavior of each feature within a component
- Keep documentation in sync with code changes—update docs whenever feature behavior changes

## Environment Variables

Copy `.env.example` to `.env`. Key variables:
- `DISCORD_TOKEN`, `DISCORD_CLIENT_ID` - Discord bot credentials
- `LIA__DATABASE__URL` - PostgreSQL connection string
- `LIA__CLAUDE__API_KEY` - Anthropic API key
- `LIA__FIRECRACKER__*` - Paths to Firecracker binaries and VM artifacts
