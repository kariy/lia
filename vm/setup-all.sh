#!/bin/bash
set -euo pipefail

# =============================================================================
# Lia VM Infrastructure Complete Setup (QEMU)
# =============================================================================
# This script sets up everything needed to run Lia VMs using QEMU:
#   1. System dependencies
#   2. QEMU installation
#   3. Kernel
#   4. Agent sidecar
#   5. Rootfs with SSH support
#   6. Network bridge and NAT
#
# Prerequisites:
#   - Ubuntu/Debian Linux with KVM support
#   - Root access
#   - Internet connection
#   - Rust installed (https://rustup.rs)
#
# Usage:
#   sudo bash vm/setup-all.sh
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Configuration
LIA_DIR="/var/lib/lia"
BRIDGE_NAME="lia-br0"
BRIDGE_IP="172.16.0.1"
BRIDGE_SUBNET="172.16.0.0/24"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# =============================================================================
# Step 0: Pre-flight checks
# =============================================================================
preflight_checks() {
    log_info "Running pre-flight checks..."

    # Check if running as root
    if [ "$EUID" -ne 0 ]; then
        log_error "This script must be run as root"
        echo "Usage: sudo bash $0"
        exit 1
    fi

    # Check for KVM support
    if [ ! -e /dev/kvm ]; then
        log_error "KVM not available. Please enable virtualization in BIOS."
        exit 1
    fi

    # Check for required commands
    local required_cmds="curl tar mkfs.ext4 ip iptables chroot"
    for cmd in $required_cmds; do
        if ! command -v $cmd &> /dev/null; then
            log_error "Required command not found: $cmd"
            exit 1
        fi
    done

    # Check for apk (needed for rootfs build)
    if ! command -v apk &> /dev/null; then
        log_warn "apk not found. Installing alpine-make-rootfs dependencies..."
        apt-get update
        apt-get install -y wget
    fi

    log_success "Pre-flight checks passed"
}

# =============================================================================
# Step 1: Install system dependencies
# =============================================================================
install_dependencies() {
    log_info "Installing system dependencies..."

    apt-get update
    apt-get install -y \
        curl \
        wget \
        git \
        build-essential \
        pkg-config \
        libssl-dev \
        qemu-system-x86 \
        qemu-utils \
        debootstrap \
        e2fsprogs \
        iptables \
        iproute2 \
        bridge-utils \
        openssh-client \
        jq

    # Source cargo env if it exists
    [ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env" 2>/dev/null || true
    [ -f "/root/.cargo/env" ] && source "/root/.cargo/env" 2>/dev/null || true

    # Install apk-tools for building Alpine rootfs
    if ! command -v apk &> /dev/null; then
        log_info "Installing apk-tools..."

        # Find the latest apk-tools-static package dynamically
        local APK_INDEX="https://dl-cdn.alpinelinux.org/alpine/v3.19/main/x86_64/"
        local APK_PACKAGE=$(curl -sL "${APK_INDEX}" | grep -o 'apk-tools-static-[^"]*\.apk' | head -1)

        if [ -z "${APK_PACKAGE}" ]; then
            log_error "Could not find apk-tools-static package"
            exit 1
        fi

        local APK_URL="${APK_INDEX}${APK_PACKAGE}"
        log_info "Downloading ${APK_PACKAGE}..."

        mkdir -p /tmp/apk-tools
        cd /tmp/apk-tools
        curl -fsSL "${APK_URL}" -o apk-tools-static.apk
        tar -xzf apk-tools-static.apk
        cp sbin/apk.static /usr/local/bin/apk
        chmod +x /usr/local/bin/apk
        cd - > /dev/null
        rm -rf /tmp/apk-tools

        log_success "apk-tools installed"
    fi

    log_success "System dependencies installed"
}

# =============================================================================
# Step 2: Create directories
# =============================================================================
create_directories() {
    log_info "Creating directories..."

    mkdir -p ${LIA_DIR}/{kernel,rootfs,volumes,sockets,logs,taps}
    mkdir -p /var/run/lia
    chmod 755 ${LIA_DIR}
    chmod 755 /var/run/lia

    log_success "Directories created at ${LIA_DIR}"
}

# =============================================================================
# Step 3: Verify QEMU installation
# =============================================================================
verify_qemu() {
    log_info "Verifying QEMU installation..."

    if ! command -v qemu-system-x86_64 &> /dev/null; then
        log_error "QEMU not found after installation"
        exit 1
    fi

    log_success "QEMU installed: $(qemu-system-x86_64 --version | head -1)"
}

# =============================================================================
# Step 4: Download/setup kernel
# =============================================================================
download_kernel() {
    log_info "Setting up kernel for QEMU..."

    local KERNEL_PATH="${LIA_DIR}/kernel/vmlinuz"

    if [ -f "${KERNEL_PATH}" ]; then
        log_info "Kernel already exists at ${KERNEL_PATH}"
        log_success "Kernel setup skipped (already exists)"
        return
    fi

    cd ${LIA_DIR}/kernel
    bash "${SCRIPT_DIR}/kernel/download-kernel.sh"

    log_success "Kernel downloaded to ${KERNEL_PATH}"
}

# =============================================================================
# Step 5: Build agent sidecar
# =============================================================================
build_agent_sidecar() {
    log_info "Building agent sidecar..."

    # Ensure cargo is in PATH (check both root and original user's cargo)
    if [ -f "$HOME/.cargo/env" ]; then
        source "$HOME/.cargo/env"
    elif [ -f "/root/.cargo/env" ]; then
        source "/root/.cargo/env"
    fi

    # Verify cargo is available
    if ! command -v cargo &> /dev/null; then
        log_error "Cargo not found. Rust installation may have failed."
        exit 1
    fi

    cd "${PROJECT_ROOT}/vm/agent-sidecar"
    cargo build --release

    log_success "Agent sidecar built at ${PROJECT_ROOT}/vm/agent-sidecar/target/release/agent-sidecar"
}

# =============================================================================
# Step 6: Build rootfs
# =============================================================================
build_rootfs() {
    log_info "Building rootfs..."

    local ROOTFS_PATH="${LIA_DIR}/rootfs/rootfs.ext4"

    if [ -f "${ROOTFS_PATH}" ]; then
        log_warn "Rootfs already exists at ${ROOTFS_PATH}"
        read -p "Do you want to rebuild it? [y/N] " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_success "Rootfs build skipped"
            return
        fi
        rm -f "${ROOTFS_PATH}"
    fi

    cd "${SCRIPT_DIR}/rootfs"
    bash build-rootfs.sh

    # Move rootfs to final location
    if [ -f "rootfs.ext4" ]; then
        mv rootfs.ext4 "${ROOTFS_PATH}"
    fi

    log_success "Rootfs built at ${ROOTFS_PATH}"
}

# =============================================================================
# Step 7: Setup networking
# =============================================================================
setup_networking() {
    log_info "Setting up networking..."

    # Set permissions for KVM and vsock
    chmod 666 /dev/kvm
    modprobe vhost_vsock 2>/dev/null || true
    chmod 666 /dev/vhost-vsock 2>/dev/null || true

    # Load TUN module for TAP devices
    modprobe tun 2>/dev/null || true

    # Check if bridge already exists
    if ip link show ${BRIDGE_NAME} &>/dev/null; then
        log_info "Bridge ${BRIDGE_NAME} already exists"
    else
        # Create bridge
        ip link add name ${BRIDGE_NAME} type bridge
        ip addr add ${BRIDGE_IP}/24 dev ${BRIDGE_NAME}
        ip link set ${BRIDGE_NAME} up
        log_success "Bridge ${BRIDGE_NAME} created with IP ${BRIDGE_IP}"
    fi

    # Enable IP forwarding
    echo 1 > /proc/sys/net/ipv4/ip_forward

    # Make IP forwarding persistent
    if ! grep -q "net.ipv4.ip_forward=1" /etc/sysctl.conf 2>/dev/null; then
        echo "net.ipv4.ip_forward=1" >> /etc/sysctl.conf
    fi

    # Configure iptables NAT
    log_info "Configuring iptables NAT..."

    # Detect primary network interface
    local PRIMARY_IF=$(ip route | grep default | awk '{print $5}' | head -1)
    log_info "Primary network interface: ${PRIMARY_IF}"

    # Clear existing rules for our subnet (to be idempotent)
    iptables -t nat -D POSTROUTING -s ${BRIDGE_SUBNET} -o ${PRIMARY_IF} -j MASQUERADE 2>/dev/null || true
    iptables -D FORWARD -i ${BRIDGE_NAME} -o ${PRIMARY_IF} -j ACCEPT 2>/dev/null || true
    iptables -D FORWARD -i ${PRIMARY_IF} -o ${BRIDGE_NAME} -m state --state RELATED,ESTABLISHED -j ACCEPT 2>/dev/null || true

    # Add NAT rules
    iptables -t nat -A POSTROUTING -s ${BRIDGE_SUBNET} -o ${PRIMARY_IF} -j MASQUERADE
    iptables -A FORWARD -i ${BRIDGE_NAME} -o ${PRIMARY_IF} -j ACCEPT
    iptables -A FORWARD -i ${PRIMARY_IF} -o ${BRIDGE_NAME} -m state --state RELATED,ESTABLISHED -j ACCEPT

    log_success "NAT configured for ${BRIDGE_SUBNET} -> ${PRIMARY_IF}"

    # Create helper scripts
    create_helper_scripts

    # Create systemd services
    create_systemd_services

    log_success "Networking setup complete"
}

# =============================================================================
# Helper: Create TAP device scripts
# =============================================================================
create_helper_scripts() {
    log_info "Creating helper scripts..."

    # Script to create TAP device for a VM
    cat > /usr/local/bin/lia-create-tap << 'TAPSCRIPT'
#!/bin/bash
set -euo pipefail

TAP_NAME=$1
BRIDGE_NAME=${2:-lia-br0}

# Create TAP device
ip tuntap add dev ${TAP_NAME} mode tap
ip link set ${TAP_NAME} up
ip link set ${TAP_NAME} master ${BRIDGE_NAME}

echo "Created TAP device ${TAP_NAME} attached to ${BRIDGE_NAME}"
TAPSCRIPT
    chmod +x /usr/local/bin/lia-create-tap

    # Script to delete TAP device
    cat > /usr/local/bin/lia-delete-tap << 'TAPSCRIPT'
#!/bin/bash
set -euo pipefail

TAP_NAME=$1

if ip link show ${TAP_NAME} &>/dev/null; then
    ip link set ${TAP_NAME} down
    ip link delete ${TAP_NAME}
    echo "Deleted TAP device ${TAP_NAME}"
else
    echo "TAP device ${TAP_NAME} does not exist"
fi
TAPSCRIPT
    chmod +x /usr/local/bin/lia-delete-tap

    log_success "Helper scripts created"
}

# =============================================================================
# Helper: Create systemd services
# =============================================================================
create_systemd_services() {
    log_info "Creating systemd services..."

    # Network bridge service
    cat > /etc/systemd/system/lia-network.service << EOF
[Unit]
Description=Lia Network Bridge Setup
After=network.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/bash -c '\\
    modprobe tun; \\
    modprobe vhost_vsock; \\
    chmod 666 /dev/vhost-vsock 2>/dev/null || true; \\
    ip link show ${BRIDGE_NAME} || ip link add name ${BRIDGE_NAME} type bridge; \\
    ip addr show ${BRIDGE_NAME} | grep -q ${BRIDGE_IP} || ip addr add ${BRIDGE_IP}/24 dev ${BRIDGE_NAME}; \\
    ip link set ${BRIDGE_NAME} up; \\
    echo 1 > /proc/sys/net/ipv4/ip_forward; \\
    PRIMARY_IF=\$(ip route | grep default | awk "{print \\\$5}" | head -1); \\
    iptables -t nat -C POSTROUTING -s ${BRIDGE_SUBNET} -o \${PRIMARY_IF} -j MASQUERADE 2>/dev/null || \\
    iptables -t nat -A POSTROUTING -s ${BRIDGE_SUBNET} -o \${PRIMARY_IF} -j MASQUERADE; \\
    iptables -C FORWARD -i ${BRIDGE_NAME} -o \${PRIMARY_IF} -j ACCEPT 2>/dev/null || \\
    iptables -A FORWARD -i ${BRIDGE_NAME} -o \${PRIMARY_IF} -j ACCEPT; \\
    iptables -C FORWARD -i \${PRIMARY_IF} -o ${BRIDGE_NAME} -m state --state RELATED,ESTABLISHED -j ACCEPT 2>/dev/null || \\
    iptables -A FORWARD -i \${PRIMARY_IF} -o ${BRIDGE_NAME} -m state --state RELATED,ESTABLISHED -j ACCEPT'
ExecStop=/bin/bash -c 'ip link set ${BRIDGE_NAME} down; ip link delete ${BRIDGE_NAME}'

[Install]
WantedBy=multi-user.target
EOF

    # VM API service
    cat > /etc/systemd/system/lia-vm-api.service << 'EOF'
[Unit]
Description=Lia VM Management API
After=network.target postgresql.service lia-network.service

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=/opt/lia
ExecStart=/opt/lia/vm-api
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

    systemctl daemon-reload
    systemctl enable lia-network.service

    log_success "Systemd services created"
}

# =============================================================================
# Step 8: Verify installation
# =============================================================================
verify_installation() {
    log_info "Verifying installation..."

    local errors=0

    # Check QEMU
    if ! command -v qemu-system-x86_64 &> /dev/null; then
        log_error "QEMU not found"
        errors=$((errors + 1))
    else
        log_success "QEMU: $(qemu-system-x86_64 --version | head -1)"
    fi

    # Check kernel
    if [ ! -f "${LIA_DIR}/kernel/vmlinuz" ]; then
        log_error "Kernel not found at ${LIA_DIR}/kernel/vmlinuz"
        errors=$((errors + 1))
    else
        log_success "Kernel: $(du -h ${LIA_DIR}/kernel/vmlinuz | cut -f1)"
    fi

    # Check rootfs
    if [ ! -f "${LIA_DIR}/rootfs/rootfs.ext4" ]; then
        log_error "Rootfs not found at ${LIA_DIR}/rootfs/rootfs.ext4"
        errors=$((errors + 1))
    else
        log_success "Rootfs: $(du -h ${LIA_DIR}/rootfs/rootfs.ext4 | cut -f1)"
    fi

    # Check agent sidecar
    if [ ! -f "${PROJECT_ROOT}/vm/agent-sidecar/target/release/agent-sidecar" ]; then
        log_error "Agent sidecar not found"
        errors=$((errors + 1))
    else
        log_success "Agent sidecar: built"
    fi

    # Check bridge
    if ! ip link show ${BRIDGE_NAME} &>/dev/null; then
        log_error "Bridge ${BRIDGE_NAME} not found"
        errors=$((errors + 1))
    else
        log_success "Bridge: ${BRIDGE_NAME} (${BRIDGE_IP})"
    fi

    # Check KVM
    if [ ! -e /dev/kvm ]; then
        log_error "KVM not available"
        errors=$((errors + 1))
    else
        log_success "KVM: available"
    fi

    # Check vhost-vsock
    if [ ! -e /dev/vhost-vsock ]; then
        log_warn "vhost-vsock not available (vsock may not work)"
    else
        log_success "vhost-vsock: available"
    fi

    if [ $errors -gt 0 ]; then
        log_error "Verification failed with ${errors} error(s)"
        return 1
    fi

    log_success "All components verified successfully!"
    return 0
}

# =============================================================================
# Main
# =============================================================================
main() {
    echo ""
    echo "============================================"
    echo "  Lia VM Infrastructure Setup (QEMU)"
    echo "============================================"
    echo ""

    preflight_checks
    install_dependencies
    create_directories
    verify_qemu
    download_kernel
    build_agent_sidecar
    build_rootfs
    setup_networking
    verify_installation

    echo ""
    echo "============================================"
    echo "  Setup Complete!"
    echo "============================================"
    echo ""
    echo "Network configuration:"
    echo "  Bridge: ${BRIDGE_NAME} (${BRIDGE_IP})"
    echo "  VM subnet: ${BRIDGE_SUBNET}"
    echo ""
    echo "Paths:"
    echo "  Kernel: ${LIA_DIR}/kernel/vmlinuz"
    echo "  Rootfs: ${LIA_DIR}/rootfs/rootfs.ext4"
    echo "  Volumes: ${LIA_DIR}/volumes/"
    echo "  Sockets: ${LIA_DIR}/sockets/"
    echo "  PIDs: /var/run/lia/"
    echo ""
    echo "Next steps:"
    echo "  1. Build the VM API: cd ${PROJECT_ROOT} && make build-api"
    echo "  2. Run the SSH integration test: make test-ssh"
    echo "  3. Configure environment variables (see .env.example)"
    echo "  4. Start the services: make dev"
    echo ""
    echo "To test network bridge:"
    echo "  ip addr show ${BRIDGE_NAME}"
    echo "  ping -c1 ${BRIDGE_IP}"
    echo ""
}

# Run main
main "$@"
