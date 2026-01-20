#!/bin/bash
set -euo pipefail

# Setup script for Lia VM infrastructure
# Run as root on the host machine

LIA_DIR="/var/lib/lia"
FIRECRACKER_VERSION="v1.6.0"
BRIDGE_NAME="lia-br0"
BRIDGE_IP="172.16.0.1"
BRIDGE_SUBNET="172.16.0.0/24"

echo "Setting up Lia VM infrastructure..."

# Check for KVM support
if [ ! -e /dev/kvm ]; then
    echo "Error: KVM not available. Please enable virtualization in BIOS."
    exit 1
fi

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Error: This script must be run as root"
    exit 1
fi

# Create directories
echo "Creating directories..."
mkdir -p ${LIA_DIR}/{kernel,rootfs,volumes,sockets,logs,taps}
chmod 755 ${LIA_DIR}

# Download Firecracker
echo "Downloading Firecracker ${FIRECRACKER_VERSION}..."
ARCH=$(uname -m)
curl -fsSL -o /tmp/firecracker.tgz \
    "https://github.com/firecracker-microvm/firecracker/releases/download/${FIRECRACKER_VERSION}/firecracker-${FIRECRACKER_VERSION}-${ARCH}.tgz"

tar -xzf /tmp/firecracker.tgz -C /tmp
mv /tmp/release-${FIRECRACKER_VERSION}-${ARCH}/firecracker-${FIRECRACKER_VERSION}-${ARCH} /usr/local/bin/firecracker
mv /tmp/release-${FIRECRACKER_VERSION}-${ARCH}/jailer-${FIRECRACKER_VERSION}-${ARCH} /usr/local/bin/jailer
chmod +x /usr/local/bin/firecracker /usr/local/bin/jailer
rm -rf /tmp/firecracker.tgz /tmp/release-${FIRECRACKER_VERSION}-${ARCH}

echo "Firecracker installed: $(firecracker --version)"

# Download kernel
echo "Downloading kernel..."
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd ${LIA_DIR}/kernel
bash "${SCRIPT_DIR}/kernel/download-kernel.sh"

# Set permissions for KVM and vsock
echo "Configuring permissions..."
chmod 666 /dev/kvm
modprobe vhost_vsock 2>/dev/null || true
chmod 666 /dev/vhost-vsock 2>/dev/null || true

# Load TUN module for TAP devices
modprobe tun 2>/dev/null || true

# ============================================
# Network Bridge Setup
# ============================================
echo "Setting up network bridge ${BRIDGE_NAME}..."

# Check if bridge already exists
if ip link show ${BRIDGE_NAME} &>/dev/null; then
    echo "Bridge ${BRIDGE_NAME} already exists, skipping creation"
else
    # Create bridge
    ip link add name ${BRIDGE_NAME} type bridge
    ip addr add ${BRIDGE_IP}/24 dev ${BRIDGE_NAME}
    ip link set ${BRIDGE_NAME} up
    echo "Bridge ${BRIDGE_NAME} created with IP ${BRIDGE_IP}"
fi

# Enable IP forwarding
echo 1 > /proc/sys/net/ipv4/ip_forward

# Make IP forwarding persistent
if ! grep -q "net.ipv4.ip_forward=1" /etc/sysctl.conf 2>/dev/null; then
    echo "net.ipv4.ip_forward=1" >> /etc/sysctl.conf
fi

# ============================================
# iptables NAT Setup
# ============================================
echo "Configuring iptables NAT..."

# Detect primary network interface (the one with default route)
PRIMARY_IF=$(ip route | grep default | awk '{print $5}' | head -1)
echo "Primary network interface: ${PRIMARY_IF}"

# Clear existing rules for our subnet (to be idempotent)
iptables -t nat -D POSTROUTING -s ${BRIDGE_SUBNET} -o ${PRIMARY_IF} -j MASQUERADE 2>/dev/null || true
iptables -D FORWARD -i ${BRIDGE_NAME} -o ${PRIMARY_IF} -j ACCEPT 2>/dev/null || true
iptables -D FORWARD -i ${PRIMARY_IF} -o ${BRIDGE_NAME} -m state --state RELATED,ESTABLISHED -j ACCEPT 2>/dev/null || true

# Add NAT rules
iptables -t nat -A POSTROUTING -s ${BRIDGE_SUBNET} -o ${PRIMARY_IF} -j MASQUERADE
iptables -A FORWARD -i ${BRIDGE_NAME} -o ${PRIMARY_IF} -j ACCEPT
iptables -A FORWARD -i ${PRIMARY_IF} -o ${BRIDGE_NAME} -m state --state RELATED,ESTABLISHED -j ACCEPT

echo "NAT configured for ${BRIDGE_SUBNET} -> ${PRIMARY_IF}"

# ============================================
# Create helper scripts
# ============================================

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

# ============================================
# Systemd service for bridge persistence
# ============================================
cat > /etc/systemd/system/lia-network.service << EOF
[Unit]
Description=Lia Network Bridge Setup
After=network.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/bash -c '\
    modprobe tun; \
    ip link show ${BRIDGE_NAME} || ip link add name ${BRIDGE_NAME} type bridge; \
    ip addr show ${BRIDGE_NAME} | grep -q ${BRIDGE_IP} || ip addr add ${BRIDGE_IP}/24 dev ${BRIDGE_NAME}; \
    ip link set ${BRIDGE_NAME} up; \
    echo 1 > /proc/sys/net/ipv4/ip_forward; \
    PRIMARY_IF=\$(ip route | grep default | awk "{print \\\$5}" | head -1); \
    iptables -t nat -C POSTROUTING -s ${BRIDGE_SUBNET} -o \${PRIMARY_IF} -j MASQUERADE 2>/dev/null || \
    iptables -t nat -A POSTROUTING -s ${BRIDGE_SUBNET} -o \${PRIMARY_IF} -j MASQUERADE; \
    iptables -C FORWARD -i ${BRIDGE_NAME} -o \${PRIMARY_IF} -j ACCEPT 2>/dev/null || \
    iptables -A FORWARD -i ${BRIDGE_NAME} -o \${PRIMARY_IF} -j ACCEPT; \
    iptables -C FORWARD -i \${PRIMARY_IF} -o ${BRIDGE_NAME} -m state --state RELATED,ESTABLISHED -j ACCEPT 2>/dev/null || \
    iptables -A FORWARD -i \${PRIMARY_IF} -o ${BRIDGE_NAME} -m state --state RELATED,ESTABLISHED -j ACCEPT'
ExecStop=/bin/bash -c 'ip link set ${BRIDGE_NAME} down; ip link delete ${BRIDGE_NAME}'

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable lia-network.service

# ============================================
# Create systemd service for VM API
# ============================================
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

echo ""
echo "============================================"
echo "Setup complete!"
echo "============================================"
echo ""
echo "Network configuration:"
echo "  Bridge: ${BRIDGE_NAME} (${BRIDGE_IP})"
echo "  VM subnet: ${BRIDGE_SUBNET}"
echo "  NAT: ${BRIDGE_SUBNET} -> ${PRIMARY_IF}"
echo ""
echo "Next steps:"
echo "1. Build the rootfs: cd vm/rootfs && sudo bash build-rootfs.sh"
echo "2. Copy rootfs: sudo cp vm/rootfs/rootfs.ext4 ${LIA_DIR}/rootfs/"
echo "3. Build the agent sidecar: cd vm/agent-sidecar && cargo build --release"
echo "4. Configure PostgreSQL and create the 'lia' database"
echo "5. Set environment variables (see .env.example)"
echo "6. Start the VM API: cargo run --release"
echo ""
echo "To test network bridge:"
echo "  ip addr show ${BRIDGE_NAME}"
echo "  ping -c1 ${BRIDGE_IP}"
