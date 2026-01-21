#!/bin/bash
set -euo pipefail

# Download or copy a kernel suitable for QEMU VMs
# QEMU can use standard Linux kernels (vmlinuz format)
# Unlike Firecracker, QEMU doesn't require special minimal kernels

OUTPUT_FILE="vmlinuz"

echo "Setting up kernel for QEMU..."

# Option 1: Use the system kernel if available
if [ -f /boot/vmlinuz-$(uname -r) ]; then
    echo "Using system kernel: /boot/vmlinuz-$(uname -r)"
    cp /boot/vmlinuz-$(uname -r) ${OUTPUT_FILE}
    chmod 644 ${OUTPUT_FILE}
    echo "Kernel copied: ${OUTPUT_FILE}"
    echo "Size: $(du -h ${OUTPUT_FILE} | cut -f1)"
    file ${OUTPUT_FILE}
    exit 0
fi

# Option 2: Download Ubuntu cloud kernel
# This is a well-tested kernel for cloud/VM use
ARCH=$(uname -m)
if [ "${ARCH}" = "x86_64" ]; then
    # Use Ubuntu's cloud kernel from their archive
    # These are optimized for VM environments
    KERNEL_VERSION="6.5.0-44-generic"
    KERNEL_URL="https://kernel.ubuntu.com/mainline/v6.5/amd64/linux-image-unsigned-6.5.0-060500-generic_6.5.0-060500.202308271531_amd64.deb"

    echo "Downloading Ubuntu kernel..."

    # Create temp directory
    TEMP_DIR=$(mktemp -d)
    cd ${TEMP_DIR}

    # Try to download from Ubuntu kernel mainline
    if curl -fsSL -o kernel.deb "${KERNEL_URL}" 2>/dev/null; then
        # Extract kernel from deb
        ar x kernel.deb
        tar -xf data.tar.* 2>/dev/null || tar -xJf data.tar.xz 2>/dev/null

        # Find vmlinuz
        VMLINUZ=$(find . -name 'vmlinuz-*' -type f | head -1)
        if [ -n "${VMLINUZ}" ]; then
            cp "${VMLINUZ}" "${OLDPWD}/${OUTPUT_FILE}"
            cd "${OLDPWD}"
            rm -rf ${TEMP_DIR}
            chmod 644 ${OUTPUT_FILE}
            echo "Kernel downloaded: ${OUTPUT_FILE}"
            echo "Size: $(du -h ${OUTPUT_FILE} | cut -f1)"
            file ${OUTPUT_FILE}
            exit 0
        fi
    fi

    cd "${OLDPWD}"
    rm -rf ${TEMP_DIR}
fi

# Option 3: Fallback - use Firecracker's quickstart kernel
# This still works with QEMU, though it's optimized for Firecracker
echo "Falling back to Firecracker quickstart kernel..."
KERNEL_URL="https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin"

curl -fsSL -o vmlinux ${KERNEL_URL}
# Rename to vmlinuz for consistency (it's the same format for our use)
mv vmlinux ${OUTPUT_FILE}

chmod 644 ${OUTPUT_FILE}

echo "Kernel downloaded: ${OUTPUT_FILE}"
echo "Size: $(du -h ${OUTPUT_FILE} | cut -f1)"

# Verify it's a valid kernel
file ${OUTPUT_FILE}
