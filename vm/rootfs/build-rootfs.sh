#!/bin/bash
set -euo pipefail

# Build a minimal Debian Linux rootfs for running Claude Code in QEMU VMs
# Uses Debian instead of Alpine because Claude Code requires glibc
# This rootfs is compatible with both QEMU and Firecracker hypervisors

ROOTFS_SIZE="2G"
ROOTFS_FILE="rootfs.ext4"
MOUNT_DIR="/tmp/lia-rootfs"
DEBIAN_VERSION="bookworm"
DEBIAN_MIRROR="http://deb.debian.org/debian"

echo "Building Lia rootfs (Debian ${DEBIAN_VERSION})..."

# Create sparse file
echo "Creating ${ROOTFS_SIZE} sparse file..."
truncate -s ${ROOTFS_SIZE} ${ROOTFS_FILE}
mkfs.ext4 -F ${ROOTFS_FILE}

# Mount the rootfs
mkdir -p ${MOUNT_DIR}
mount -o loop ${ROOTFS_FILE} ${MOUNT_DIR}

# Cleanup on exit
cleanup() {
    # Kill any processes using the mount
    fuser -k ${MOUNT_DIR} 2>/dev/null || true
    sleep 1
    umount ${MOUNT_DIR}/dev/pts 2>/dev/null || true
    umount ${MOUNT_DIR}/dev 2>/dev/null || true
    umount ${MOUNT_DIR}/sys 2>/dev/null || true
    umount ${MOUNT_DIR}/proc 2>/dev/null || true
    umount ${MOUNT_DIR} 2>/dev/null || true
    rmdir ${MOUNT_DIR} 2>/dev/null || true
}
trap cleanup EXIT

# Install Debian base using debootstrap
echo "Installing Debian ${DEBIAN_VERSION} base system..."
debootstrap --arch=amd64 --variant=minbase ${DEBIAN_VERSION} ${MOUNT_DIR} ${DEBIAN_MIRROR}

# Configure apt sources
cat > ${MOUNT_DIR}/etc/apt/sources.list << EOF
deb ${DEBIAN_MIRROR} ${DEBIAN_VERSION} main contrib
deb ${DEBIAN_MIRROR} ${DEBIAN_VERSION}-updates main contrib
deb http://security.debian.org/debian-security ${DEBIAN_VERSION}-security main contrib
EOF

# Configure DNS
cat > ${MOUNT_DIR}/etc/resolv.conf << 'EOF'
nameserver 8.8.8.8
nameserver 8.8.4.4
EOF

# Mount virtual filesystems for chroot
mount --bind /proc ${MOUNT_DIR}/proc
mount --bind /sys ${MOUNT_DIR}/sys
mount --bind /dev ${MOUNT_DIR}/dev
mount --bind /dev/pts ${MOUNT_DIR}/dev/pts

# Install required packages
echo "Installing packages..."
chroot ${MOUNT_DIR} /bin/bash -c "
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends \
        systemd systemd-sysv \
        init \
        openssh-server \
        nodejs npm \
        git curl wget ca-certificates \
        python3 python3-pip \
        build-essential \
        iproute2 iputils-ping net-tools \
        procps sudo locales \
        haveged \
        kmod

    # Enable haveged for entropy (important for crypto operations in VM)
    systemctl enable haveged

    # Clean up apt cache
    apt-get clean
    rm -rf /var/lib/apt/lists/*
"

# Configure locale
chroot ${MOUNT_DIR} /bin/bash -c "
    echo 'en_US.UTF-8 UTF-8' >> /etc/locale.gen
    locale-gen
"

# Configure systemd for VM environment (no unnecessary services)
echo "Configuring systemd..."

# Disable unnecessary services
chroot ${MOUNT_DIR} /bin/bash -c "
    systemctl mask systemd-resolved.service
    systemctl mask systemd-networkd-wait-online.service
    systemctl mask systemd-timesyncd.service
    systemctl mask apt-daily.timer
    systemctl mask apt-daily-upgrade.timer
    systemctl mask e2scrub_all.timer
    systemctl mask fstrim.timer
"

# Configure serial console
mkdir -p ${MOUNT_DIR}/etc/systemd/system/serial-getty@ttyS0.service.d
cat > ${MOUNT_DIR}/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf << 'EOF'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin root -o '-p -- \\u' --keep-baud 115200,38400,9600 %I $TERM
EOF

# Enable serial console
chroot ${MOUNT_DIR} /bin/bash -c "
    systemctl enable serial-getty@ttyS0.service
"

# Configure SSH server
echo "Configuring SSH server..."
cat > ${MOUNT_DIR}/etc/ssh/sshd_config << 'EOF'
Port 22
AddressFamily any
ListenAddress 0.0.0.0

# Authentication
PermitRootLogin yes
PasswordAuthentication no
PubkeyAuthentication yes
AuthorizedKeysFile .ssh/authorized_keys

# Security
PermitEmptyPasswords no
ChallengeResponseAuthentication no
UsePAM no

# Features
X11Forwarding no
PrintMotd no
AcceptEnv LANG LC_*

# Keep connections alive
ClientAliveInterval 60
ClientAliveCountMax 3

# Subsystems
Subsystem sftp /usr/lib/openssh/sftp-server
EOF

# Enable SSH
chroot ${MOUNT_DIR} /bin/bash -c "systemctl enable ssh"

# Create .ssh directory for root
mkdir -p ${MOUNT_DIR}/root/.ssh
chmod 700 ${MOUNT_DIR}/root/.ssh
touch ${MOUNT_DIR}/root/.ssh/authorized_keys
chmod 600 ${MOUNT_DIR}/root/.ssh/authorized_keys

# Create claude user for running Claude Code (cannot use --dangerously-skip-permissions as root)
echo "Creating claude user..."
chroot ${MOUNT_DIR} /bin/bash -c "
    useradd -m -s /bin/bash claude
    mkdir -p /home/claude/.local/bin
    chown -R claude:claude /home/claude
"

# Install Claude Code
echo "Installing Claude Code..."
chroot ${MOUNT_DIR} /bin/bash -c "
    export HOME=/root
    curl -fsSL https://claude.ai/install.sh | bash
"

# Verify Claude installation and copy to shared location
if chroot ${MOUNT_DIR} /bin/bash -c "test -f /root/.local/bin/claude"; then
    echo "Claude Code installed successfully"

    # Get the actual binary path (claude is a symlink)
    CLAUDE_TARGET=$(chroot ${MOUNT_DIR} readlink -f /root/.local/bin/claude)
    echo "Claude binary at: ${CLAUDE_TARGET}"

    # Copy Claude to a shared location accessible by all users
    mkdir -p ${MOUNT_DIR}/usr/local/share/claude/versions
    cp ${MOUNT_DIR}${CLAUDE_TARGET} ${MOUNT_DIR}/usr/local/share/claude/versions/
    chmod 755 ${MOUNT_DIR}/usr/local/share/claude/versions/*

    # Create symlinks for both root and claude user
    CLAUDE_VERSION=$(basename ${CLAUDE_TARGET})
    ln -sf /usr/local/share/claude/versions/${CLAUDE_VERSION} ${MOUNT_DIR}/root/.local/bin/claude 2>/dev/null || true
    ln -sf /usr/local/share/claude/versions/${CLAUDE_VERSION} ${MOUNT_DIR}/home/claude/.local/bin/claude
    chown -R claude:claude ${MOUNT_DIR}/home/claude/.local/bin

    # Add to PATH system-wide
    echo 'export PATH="/home/claude/.local/bin:/usr/local/bin:$PATH"' >> ${MOUNT_DIR}/etc/profile.d/claude.sh
else
    echo "Warning: Claude Code installation may have failed"
fi

# Configure sudo to allow root to run commands as claude without password
cat > ${MOUNT_DIR}/etc/sudoers.d/agent-sidecar << 'EOF'
# Allow root to run commands as claude without password
root ALL=(claude) NOPASSWD: ALL
EOF
chmod 440 ${MOUNT_DIR}/etc/sudoers.d/agent-sidecar

# Copy agent sidecar binary
# Prefer musl build for portability (works regardless of glibc version)
SIDECAR_MUSL_PATH="../agent-sidecar/target/x86_64-unknown-linux-musl/release/agent-sidecar"
SIDECAR_PATH="../agent-sidecar/target/release/agent-sidecar"

if [ -f "${SIDECAR_MUSL_PATH}" ]; then
    echo "Copying agent-sidecar (musl build - recommended for portability)..."
    cp ${SIDECAR_MUSL_PATH} ${MOUNT_DIR}/usr/local/bin/
    chmod +x ${MOUNT_DIR}/usr/local/bin/agent-sidecar
elif [ -f "${SIDECAR_PATH}" ]; then
    echo "Copying agent-sidecar (glibc build - may have version compatibility issues)..."
    cp ${SIDECAR_PATH} ${MOUNT_DIR}/usr/local/bin/
    chmod +x ${MOUNT_DIR}/usr/local/bin/agent-sidecar
else
    echo "Warning: agent-sidecar binary not found, skipping..."
    echo "  Build with: cd ../agent-sidecar && cargo build --release --target x86_64-unknown-linux-musl"
fi

# Create workspace directory (owned by claude user for file operations)
mkdir -p ${MOUNT_DIR}/workspace
chroot ${MOUNT_DIR} /bin/bash -c "chown claude:claude /workspace"
chmod 755 ${MOUNT_DIR}/workspace

# Create network configuration script (run at boot)
cat > ${MOUNT_DIR}/usr/local/bin/lia-network-init << 'EOF'
#!/bin/bash
# Configure networking from kernel command line parameters
# Format: lia.ip=172.16.0.X lia.gateway=172.16.0.1 lia.ssh_key="ssh-rsa ..."

CMDLINE=$(cat /proc/cmdline)

# Extract parameters
IP=$(echo "$CMDLINE" | tr ' ' '\n' | grep '^lia.ip=' | cut -d= -f2)
GATEWAY=$(echo "$CMDLINE" | tr ' ' '\n' | grep '^lia.gateway=' | cut -d= -f2)
SSH_KEY=$(echo "$CMDLINE" | tr ' ' '\n' | grep '^lia.ssh_key=' | cut -d= -f2- | sed 's/+/ /g')

if [ -n "$IP" ]; then
    echo "Configuring IP: $IP"
    ip addr add ${IP}/24 dev eth0 2>/dev/null || true
    ip link set eth0 up
    ip route add default via ${GATEWAY:-172.16.0.1} 2>/dev/null || true
fi

if [ -n "$SSH_KEY" ]; then
    echo "Configuring SSH key"
    mkdir -p /root/.ssh
    echo "$SSH_KEY" > /root/.ssh/authorized_keys
    chmod 600 /root/.ssh/authorized_keys
fi
EOF
chmod +x ${MOUNT_DIR}/usr/local/bin/lia-network-init

# Create systemd service for network init
cat > ${MOUNT_DIR}/etc/systemd/system/lia-network-init.service << 'EOF'
[Unit]
Description=Lia Network Initialization
Before=network.target ssh.service
After=systemd-udevd.service

[Service]
Type=oneshot
ExecStart=/usr/local/bin/lia-network-init
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOF
chroot ${MOUNT_DIR} /bin/bash -c "systemctl enable lia-network-init.service"

# Install kernel modules for vsock (required for QEMU vhost-vsock-pci)
echo "Installing kernel modules for vsock..."
KERNEL_VERSION=$(uname -r)
MODULES_SRC="/lib/modules/${KERNEL_VERSION}/kernel/net/vmw_vsock"

if [ -d "${MODULES_SRC}" ]; then
    mkdir -p ${MOUNT_DIR}/lib/modules/${KERNEL_VERSION}/kernel/net/vmw_vsock

    # Copy and decompress vsock modules
    for mod in ${MODULES_SRC}/*.ko.zst; do
        if [ -f "$mod" ]; then
            modname=$(basename "$mod" .zst)
            zstd -d "$mod" -o "${MOUNT_DIR}/lib/modules/${KERNEL_VERSION}/kernel/net/vmw_vsock/${modname}" 2>/dev/null || \
                cp "$mod" "${MOUNT_DIR}/lib/modules/${KERNEL_VERSION}/kernel/net/vmw_vsock/"
        fi
    done

    # Also copy uncompressed modules if they exist
    for mod in ${MODULES_SRC}/*.ko; do
        if [ -f "$mod" ]; then
            cp "$mod" "${MOUNT_DIR}/lib/modules/${KERNEL_VERSION}/kernel/net/vmw_vsock/"
        fi
    done

    # Copy modules.* metadata files
    cp /lib/modules/${KERNEL_VERSION}/modules.builtin* ${MOUNT_DIR}/lib/modules/${KERNEL_VERSION}/ 2>/dev/null || true
    cp /lib/modules/${KERNEL_VERSION}/modules.order ${MOUNT_DIR}/lib/modules/${KERNEL_VERSION}/ 2>/dev/null || true

    # Generate modules.dep
    chroot ${MOUNT_DIR} /sbin/depmod -a ${KERNEL_VERSION} 2>/dev/null || true

    echo "vsock kernel modules installed for kernel ${KERNEL_VERSION}"
else
    echo "Warning: vsock kernel modules not found at ${MODULES_SRC}"
    echo "The VM may not be able to communicate via vsock"
fi

# Create systemd service for loading vsock modules at boot
cat > ${MOUNT_DIR}/etc/systemd/system/vsock-modules.service << 'EOF'
[Unit]
Description=Load vsock kernel modules
DefaultDependencies=no
Before=sysinit.target

[Service]
Type=oneshot
ExecStart=/sbin/modprobe vsock
ExecStart=/sbin/modprobe vmw_vsock_virtio_transport
RemainAfterExit=yes

[Install]
WantedBy=sysinit.target
EOF
chroot ${MOUNT_DIR} /bin/bash -c "systemctl enable vsock-modules.service"

# Create systemd service for agent-sidecar
cat > ${MOUNT_DIR}/etc/systemd/system/agent-sidecar.service << 'EOF'
[Unit]
Description=Lia Agent Sidecar
After=network.target lia-network-init.service vsock-modules.service
Requires=vsock-modules.service

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
EOF
chroot ${MOUNT_DIR} /bin/bash -c "systemctl enable agent-sidecar.service"

# Set hostname
echo "lia-agent" > ${MOUNT_DIR}/etc/hostname

# Configure hosts file
cat > ${MOUNT_DIR}/etc/hosts << 'EOF'
127.0.0.1   localhost
127.0.1.1   lia-agent
EOF

# Set root password (empty for passwordless console login)
chroot ${MOUNT_DIR} /bin/bash -c "passwd -d root"

# Create profile for environment
cat > ${MOUNT_DIR}/etc/profile.d/lia.sh << 'EOF'
export PATH="/home/claude/.local/bin:/usr/local/bin:$PATH"
export TERM=xterm-256color
cd /workspace 2>/dev/null || true
EOF

# Configure git for both root and claude users
for GITCONFIG in ${MOUNT_DIR}/root/.gitconfig ${MOUNT_DIR}/home/claude/.gitconfig; do
    cat > ${GITCONFIG} << 'EOF'
[user]
    name = Lia Agent
    email = agent@lia.local
[init]
    defaultBranch = main
[safe]
    directory = /workspace
EOF
done
chroot ${MOUNT_DIR} /bin/bash -c "chown claude:claude /home/claude/.gitconfig"

# Unmount virtual filesystems
umount ${MOUNT_DIR}/dev/pts
umount ${MOUNT_DIR}/dev
umount ${MOUNT_DIR}/sys
umount ${MOUNT_DIR}/proc

echo ""
echo "Rootfs build complete: ${ROOTFS_FILE}"
echo "Size: $(du -h ${ROOTFS_FILE} | cut -f1)"
echo ""
echo "To install to /var/lib/lia/rootfs/:"
echo "  sudo cp ${ROOTFS_FILE} /var/lib/lia/rootfs/"
