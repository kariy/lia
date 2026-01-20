#!/bin/bash
set -euo pipefail

# Download pre-built Firecracker kernel
# Alternatively, you can build your own with the config in this directory

# Use the quickstart kernel from Firecracker
KERNEL_URL="https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin"
OUTPUT_FILE="vmlinux"

echo "Downloading Firecracker kernel..."

curl -fsSL -o ${OUTPUT_FILE} ${KERNEL_URL}

chmod 644 ${OUTPUT_FILE}

echo "Kernel downloaded: ${OUTPUT_FILE}"
echo "Size: $(du -h ${OUTPUT_FILE} | cut -f1)"

# Verify it's a valid kernel
file ${OUTPUT_FILE}
