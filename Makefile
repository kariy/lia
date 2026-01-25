.PHONY: all build install dev dev-api dev-web dev-bot clean setup setup-all test test-ssh dev-web-remote build-cli install-cli

all: build

# Build all components
build: build-api build-shared build-web build-bot

build-api:
	cd api && cargo build --release

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
	cd api && cargo run

dev-web:
	cd packages/web-ui && bun run dev

dev-web-remote:
	cd packages/web-ui && bun run dev --host 0.0.0.0

dev-bot:
	cd packages/discord-bot && bun --env-file=../../.env run dev

# Deploy Discord commands
deploy-commands:
	cd packages/discord-bot && bun --env-file=../../.env run deploy-commands

# Clean build artifacts
clean:
	cd api && cargo clean
	rm -rf packages/web-ui/dist
	rm -rf packages/discord-bot/dist
	rm -rf node_modules

# Database
db-migrate:
	cd api && sqlx migrate run

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
	cd api && cargo check

# Run tests
test:
	cd api && cargo test
	cd vm/agent-sidecar-python && python3 -m unittest test_agent_sidecar -v

# Run SSH integration test (requires root and infrastructure setup)
# Prerequisites: sudo bash vm/setup.sh && sudo bash vm/rootfs/build-rootfs.sh
test-ssh:
	cd api && sudo cargo test --test ssh_integration_test -- --nocapture --test-threads=1

# Run Claude streaming integration test (requires root, infrastructure, and API key)
# Prerequisites: sudo bash vm/setup.sh && sudo bash vm/rootfs/build-rootfs.sh
# Usage: make test-claude ANTHROPIC_API_KEY=sk-...
test-claude:
	cd api && sudo ANTHROPIC_API_KEY=$(ANTHROPIC_API_KEY) cargo test --test claude_streaming_test -- --nocapture --test-threads=1

# Build CLI tool
build-cli:
	cd cli && cargo build --release

# Install CLI tool to ~/.local/bin
install-cli: build-cli
	mkdir -p ~/.local/bin
	cp cli/target/release/lia ~/.local/bin/
