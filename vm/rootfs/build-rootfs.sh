#!/bin/bash
set -euo pipefail

# Build a minimal Alpine Linux rootfs for running Claude Code in Firecracker

ROOTFS_SIZE="2G"
ROOTFS_FILE="rootfs.ext4"
MOUNT_DIR="/tmp/lia-rootfs"
ALPINE_VERSION="3.19"
ALPINE_MIRROR="https://dl-cdn.alpinelinux.org/alpine"

echo "Building Lia rootfs..."

# Create sparse file
echo "Creating ${ROOTFS_SIZE} sparse file..."
truncate -s ${ROOTFS_SIZE} ${ROOTFS_FILE}
mkfs.ext4 -F ${ROOTFS_FILE}

# Mount the rootfs
mkdir -p ${MOUNT_DIR}
mount -o loop ${ROOTFS_FILE} ${MOUNT_DIR}

# Cleanup on exit
cleanup() {
    umount ${MOUNT_DIR}/dev 2>/dev/null || true
    umount ${MOUNT_DIR}/sys 2>/dev/null || true
    umount ${MOUNT_DIR}/proc 2>/dev/null || true
    umount ${MOUNT_DIR} 2>/dev/null || true
    rmdir ${MOUNT_DIR} 2>/dev/null || true
}
trap cleanup EXIT

# Install Alpine base
echo "Installing Alpine Linux ${ALPINE_VERSION}..."
apk --arch x86_64 -X ${ALPINE_MIRROR}/v${ALPINE_VERSION}/main \
    -U --allow-untrusted --root ${MOUNT_DIR} --initdb \
    add alpine-base openrc busybox busybox-binsh

# Create mount points for virtual filesystems
mkdir -p ${MOUNT_DIR}/{proc,sys,dev}

# Configure Alpine
cat > ${MOUNT_DIR}/etc/inittab << 'EOF'
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default

# Set up a console on ttyS0
ttyS0::respawn:/sbin/getty -L ttyS0 115200 vt100

::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
EOF

# Configure networking - will be configured by init script based on metadata
cat > ${MOUNT_DIR}/etc/network/interfaces << 'EOF'
auto lo
iface lo inet loopback

auto eth0
iface eth0 inet static
    address 172.16.0.2
    netmask 255.255.255.0
    gateway 172.16.0.1
EOF

# Enable services
mkdir -p ${MOUNT_DIR}/etc/runlevels/default
mkdir -p ${MOUNT_DIR}/etc/runlevels/boot
ln -sf /etc/init.d/networking ${MOUNT_DIR}/etc/runlevels/default/networking

# Configure DNS early (needed for chroot network access)
cat > ${MOUNT_DIR}/etc/resolv.conf << 'EOF'
nameserver 8.8.8.8
nameserver 8.8.4.4
EOF

# Install required packages including SSH server
echo "Installing packages..."
apk --arch x86_64 -X ${ALPINE_MIRROR}/v${ALPINE_VERSION}/main \
    -X ${ALPINE_MIRROR}/v${ALPINE_VERSION}/community \
    --allow-untrusted --root ${MOUNT_DIR} add \
    nodejs npm git curl wget bash \
    openssh-server openssh-client \
    python3 py3-pip build-base linux-headers \
    ca-certificates tzdata sudo

# Configure SSH server
echo "Configuring SSH server..."
mkdir -p ${MOUNT_DIR}/etc/ssh
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
Subsystem sftp /usr/lib/ssh/sftp-server
EOF

# Generate SSH host keys
echo "Generating SSH host keys..."
chroot ${MOUNT_DIR} /bin/sh -c "ssh-keygen -A"

# Enable SSH service
ln -sf /etc/init.d/sshd ${MOUNT_DIR}/etc/runlevels/default/sshd

# Create .ssh directory for root
mkdir -p ${MOUNT_DIR}/root/.ssh
chmod 700 ${MOUNT_DIR}/root/.ssh
touch ${MOUNT_DIR}/root/.ssh/authorized_keys
chmod 600 ${MOUNT_DIR}/root/.ssh/authorized_keys

# Install Claude Code
echo "Installing Claude Code..."
# Bind mount virtual filesystems needed by the installer
mount --bind /proc ${MOUNT_DIR}/proc
mount --bind /sys ${MOUNT_DIR}/sys
mount --bind /dev ${MOUNT_DIR}/dev

chroot ${MOUNT_DIR} /bin/sh -c "curl -fsSL https://claude.ai/install.sh | bash"

# Unmount virtual filesystems
umount ${MOUNT_DIR}/dev
umount ${MOUNT_DIR}/sys
umount ${MOUNT_DIR}/proc

# Copy agent sidecar binary (must be built first)
if [ -f "../agent-sidecar/target/release/agent-sidecar" ]; then
    cp ../agent-sidecar/target/release/agent-sidecar ${MOUNT_DIR}/usr/local/bin/
    chmod +x ${MOUNT_DIR}/usr/local/bin/agent-sidecar
else
    echo "Warning: agent-sidecar binary not found, skipping..."
fi

# Create workspace directory
mkdir -p ${MOUNT_DIR}/workspace
chmod 755 ${MOUNT_DIR}/workspace

# Create init script to configure networking and SSH keys from metadata
cat > ${MOUNT_DIR}/etc/init.d/lia-init << 'EOF'
#!/sbin/openrc-run

name="lia-init"
description="Initialize Lia VM networking and SSH"

depend() {
    before networking sshd
}

start() {
    ebegin "Configuring Lia VM"

    # Read metadata from kernel command line or vsock
    # The IP address and SSH key will be passed via kernel boot args
    # Format: lia.ip=172.16.0.X lia.gateway=172.16.0.1 lia.ssh_key="ssh-rsa ..."

    local ip=$(cat /proc/cmdline | tr ' ' '\n' | grep '^lia.ip=' | cut -d= -f2)
    local gateway=$(cat /proc/cmdline | tr ' ' '\n' | grep '^lia.gateway=' | cut -d= -f2)
    local ssh_key=$(cat /proc/cmdline | tr ' ' '\n' | grep '^lia.ssh_key=' | cut -d= -f2- | sed 's/+/ /g')

    if [ -n "$ip" ]; then
        cat > /etc/network/interfaces << NETEOF
auto lo
iface lo inet loopback

auto eth0
iface eth0 inet static
    address ${ip}
    netmask 255.255.255.0
    gateway ${gateway:-172.16.0.1}
NETEOF
        echo "Configured IP: ${ip}"
    fi

    if [ -n "$ssh_key" ]; then
        echo "$ssh_key" > /root/.ssh/authorized_keys
        chmod 600 /root/.ssh/authorized_keys
        echo "Configured SSH key"
    fi

    eend 0
}
EOF
chmod +x ${MOUNT_DIR}/etc/init.d/lia-init
ln -sf /etc/init.d/lia-init ${MOUNT_DIR}/etc/runlevels/boot/lia-init

# Create init script to start agent sidecar
cat > ${MOUNT_DIR}/etc/init.d/agent-sidecar << 'EOF'
#!/sbin/openrc-run

name="agent-sidecar"
command="/usr/local/bin/agent-sidecar"
command_background="yes"
pidfile="/var/run/agent-sidecar.pid"
output_log="/var/log/agent-sidecar.log"
error_log="/var/log/agent-sidecar.log"

depend() {
    need net
}
EOF
chmod +x ${MOUNT_DIR}/etc/init.d/agent-sidecar
ln -sf /etc/init.d/agent-sidecar ${MOUNT_DIR}/etc/runlevels/default/agent-sidecar

# Set hostname
echo "lia-agent" > ${MOUNT_DIR}/etc/hostname

# Set root password (empty for passwordless login on console)
chroot ${MOUNT_DIR} /bin/sh -c "passwd -d root"

# Create a simple profile
cat > ${MOUNT_DIR}/etc/profile.d/lia.sh << 'EOF'
export PATH="/usr/local/bin:$PATH"
export TERM=xterm-256color
cd /workspace 2>/dev/null || true
EOF

# Configure git
cat > ${MOUNT_DIR}/root/.gitconfig << 'EOF'
[user]
    name = Lia Agent
    email = agent@lia.local
[init]
    defaultBranch = main
[safe]
    directory = /workspace
EOF

echo "Rootfs build complete: ${ROOTFS_FILE}"
echo "Size: $(du -h ${ROOTFS_FILE} | cut -f1)"
