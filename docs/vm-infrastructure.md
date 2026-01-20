# VM Infrastructure

The VM infrastructure provides the foundation for running Firecracker microVMs that host Claude Code agents. This includes kernel setup, rootfs building, network configuration, and helper scripts.

## Directory Structure

```
vm/
├── setup.sh              # Base infrastructure setup
├── setup-all.sh          # Complete end-to-end setup
├── rootfs/
│   └── build-rootfs.sh   # Alpine rootfs build script
├── kernel/
│   └── download-kernel.sh # Kernel download script
└── agent-sidecar/        # See agent-sidecar.md
```

## Runtime Directory Structure

```
/var/lib/lia/
├── kernel/vmlinux       # Firecracker kernel
├── rootfs/rootfs.ext4   # Alpine filesystem template
├── volumes/             # Per-VM disk storage
├── sockets/             # vsock Unix sockets
├── logs/                # VM logs
└── taps/                # TAP device info
```

## Kernel Setup

**Script**: `vm/kernel/download-kernel.sh`

Downloads pre-built Firecracker kernel from AWS S3:
- **Source**: `s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin`
- **Output**: `/var/lib/lia/kernel/vmlinux`
- **Permissions**: 644

## Rootfs Build

**Script**: `vm/rootfs/build-rootfs.sh`

Creates a minimal Alpine Linux ext4 filesystem for Firecracker VMs.

### Build Parameters

| Parameter | Value |
|-----------|-------|
| Size | 2GB sparse file |
| Format | ext4 |
| Alpine Version | 3.19 |
| Mount Point | /tmp/lia-rootfs |

### Installed Packages

**Base System**:
- alpine-base, openrc, busybox-initscripts

**Development**:
- nodejs, npm, git, curl, wget, bash
- python3, py3-pip, build-base, linux-headers
- ca-certificates, tzdata, sudo

**SSH**:
- openssh-server, openssh-client

**Claude Code**:
- @anthropic-ai/claude-code (via npm)

### SSH Configuration

Location: `/etc/ssh/sshd_config`

| Setting | Value |
|---------|-------|
| PermitRootLogin | yes |
| PasswordAuthentication | no |
| PubkeyAuthentication | yes |
| AuthorizedKeysFile | .ssh/authorized_keys |
| PermitEmptyPasswords | no |
| UsePAM | no |
| ClientAliveInterval | 60 |
| ClientAliveCountMax | 3 |

### Init System

OpenRC-based initialization with custom services:

**lia-init** (`/etc/init.d/lia-init`):
- Configures networking from kernel command line parameters
- Sets up SSH authorized_keys from kernel parameter
- Parameters: `lia.ip`, `lia.gateway`, `lia.ssh_key`

**agent-sidecar** (`/etc/init.d/agent-sidecar`):
- Starts the agent-sidecar service
- Depends on network being up
- Logs to /var/log/agent-sidecar.log

### VM Environment

| Setting | Value |
|---------|-------|
| Hostname | lia-agent |
| DNS | 8.8.8.8, 8.8.4.4 |
| Console | ttyS0 at 115200 baud |
| Working Directory | /workspace |

## Network Architecture

### Bridge Configuration

| Component | Value |
|-----------|-------|
| Bridge Name | lia-br0 |
| Bridge IP | 172.16.0.1/24 |
| VM Subnet | 172.16.0.0/24 |
| VM IP Range | 172.16.0.100-254 |

### Network Flow

```
Internet
    ↕
Primary Interface (eth0, ens*, etc.)
    ↕ (NAT masquerade)
lia-br0 (172.16.0.1)
    ↕
TAP devices (tap-vm-*)
    ↕
VM eth0 (172.16.0.x)
```

### iptables Rules

```bash
# Enable masquerade for VM subnet
iptables -t nat -A POSTROUTING -s 172.16.0.0/24 -o <primary_iface> -j MASQUERADE

# Allow forwarding to/from bridge
iptables -A FORWARD -i lia-br0 -o <primary_iface> -j ACCEPT
iptables -A FORWARD -i <primary_iface> -o lia-br0 -m state --state RELATED,ESTABLISHED -j ACCEPT
```

### TAP Device Management

**Create TAP** (`/usr/local/bin/lia-create-tap`):
```bash
ip tuntap add dev $TAP_NAME mode tap
ip link set $TAP_NAME master $BRIDGE_NAME
ip link set $TAP_NAME up
```

**Delete TAP** (`/usr/local/bin/lia-delete-tap`):
```bash
ip link set $TAP_NAME down
ip link delete $TAP_NAME
```

## Setup Scripts

### Base Setup (`vm/setup.sh`)

Configures host machine for Firecracker:

1. **Pre-flight checks**: KVM support, root access
2. **Directory creation**: /var/lib/lia subdirectories
3. **Firecracker installation**: v1.6.0 binaries
4. **Network setup**: Bridge, IP forwarding, iptables
5. **Helper scripts**: TAP create/delete
6. **Systemd services**: lia-network, lia-vm-api

### Complete Setup (`vm/setup-all.sh`)

Full end-to-end installation:

1. Pre-flight checks (KVM, root, required commands)
2. Install dependencies (Rust, system packages, apk-tools)
3. Create directories
4. Install Firecracker v1.6.0
5. Download kernel
6. Build agent sidecar
7. Build rootfs
8. Setup networking
9. Verify installation

## Systemd Services

### lia-network.service

Persistent network bridge setup:
```ini
[Unit]
Description=Lia Network Bridge Setup
After=network.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/local/bin/lia-setup-network

[Install]
WantedBy=multi-user.target
```

### lia-vm-api.service

VM API daemon:
```ini
[Unit]
Description=Lia VM API Service
After=network.target lia-network.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/lia
ExecStart=/opt/lia/vm-api
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

## VM Boot Parameters

Kernel command line parameters:

| Parameter | Purpose | Example |
|-----------|---------|---------|
| `lia.ip` | VM IP address | 172.16.0.100 |
| `lia.gateway` | Gateway IP | 172.16.0.1 |
| `lia.ssh_key` | SSH public key | ssh-ed25519 AAAA... |

Standard boot args:
```
console=ttyS0 reboot=k panic=1 pci=off init=/sbin/init
```

## Firecracker Configuration

Applied via Firecracker HTTP API (Unix socket):

1. **Boot Source**: Kernel path, boot arguments
2. **Machine Config**: vcpu_count, mem_size_mib
3. **Drives**: Root (rootfs), data (volume)
4. **Network**: eth0 with MAC, TAP device
5. **vsock**: Guest CID, Unix socket path
6. **Instance Start**: Boot action

## Prerequisites

### Required Commands

- curl, tar, mkfs.ext4, ip, iptables, chroot

### Required Kernel Modules

- kvm (KVM support)
- vhost_vsock (vsock communication)
- tun (TAP devices)

### System Requirements

- KVM-capable CPU (/dev/kvm)
- Root access
- Linux kernel 4.14+

## Development

```bash
# Full setup (requires root)
sudo bash vm/setup-all.sh

# Just download kernel
cd vm/kernel && sudo bash download-kernel.sh

# Just build rootfs (requires root)
cd vm/rootfs && sudo bash build-rootfs.sh

# Verify setup
ls -la /var/lib/lia/
ip link show lia-br0
```
