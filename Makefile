.PHONY: all build install dev dev-api dev-web dev-bot clean setup setup-all test test-ssh

all: build

# Build all components
build: build-api build-sidecar build-shared build-web build-bot

build-api:
	cd services/vm-api && cargo build --release

build-sidecar:
	cd vm/agent-sidecar && cargo build --release

build-shared:
	cd packages/shared && bun run build

build-web: build-shared
	cd packages/web-ui && bun run build

build-bot: build-shared
	cd packages/discord-bot && bun run build

# Install dependencies
install:
	bun install

# Development mode - all services
dev:
	mprocs

# Development mode - individual services
dev-api:
	cd services/vm-api && cargo run

dev-web:
	cd packages/web-ui && bun run dev

dev-bot:
	cd packages/discord-bot && bun --env-file=../../.env run dev

# Deploy Discord commands
deploy-commands:
	cd packages/discord-bot && bun --env-file=../../.env run deploy-commands

# Clean build artifacts
clean:
	cd services/vm-api && cargo clean
	cd vm/agent-sidecar && cargo clean
	rm -rf packages/web-ui/dist
	rm -rf packages/discord-bot/dist
	rm -rf node_modules

# Database
db-migrate:
	cd services/vm-api && sqlx migrate run

# Setup infrastructure (requires root)
# Full setup: installs Firecracker, kernel, rootfs, networking
setup-all:
	sudo bash vm/setup-all.sh

# Quick setup: just networking (assumes Firecracker/kernel/rootfs already exist)
setup:
	sudo bash vm/setup.sh

# Type checking
typecheck:
	bun run typecheck
	cd services/vm-api && cargo check
	cd vm/agent-sidecar && cargo check

# Run tests
test:
	cd services/vm-api && cargo test
	cd vm/agent-sidecar && cargo test

# Run SSH integration test (requires root and infrastructure setup)
# Prerequisites: sudo bash vm/setup.sh && sudo bash vm/rootfs/build-rootfs.sh
test-ssh:
	cd services/vm-api && sudo cargo test --test ssh_integration_test -- --nocapture --test-threads=1

# Run Claude streaming integration test (requires root, infrastructure, and API key)
# Prerequisites: sudo bash vm/setup.sh && sudo bash vm/rootfs/build-rootfs.sh
# Usage: make test-claude ANTHROPIC_API_KEY=sk-...
test-claude:
	cd services/vm-api && sudo ANTHROPIC_API_KEY=$(ANTHROPIC_API_KEY) cargo test --test claude_streaming_test -- --nocapture --test-threads=1
