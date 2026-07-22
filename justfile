# DeepX build system
# Usage: just [recipe]

set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]

default:
    @just --list

# === Development ===

# Start Tauri dev server (clean dist first)
dev: 
    cargo build -p deepx-daemon
    cd crates/deepx-tauri && pnpm tauri dev

# Build Tauri binary only
build-tauri: clean-fe
    cd crates/deepx-tauri && pnpm build
    cargo build -p deepx-daemon -p deepx-tauri
    @echo "Binaries: target/debug/deepx-desktop + target/debug/deepx-daemon"

# === Frontend ===

# Clean Vite dist + TypeScript cache (run before build/release/dev)
clean-fe:
    @echo "Cleaning dist + tsbuildinfo..."
    node -e "const fs = require('node:fs'); for (const path of ['crates/deepx-tauri/dist', 'crates/deepx-tauri/tsconfig.tsbuildinfo']) fs.rmSync(path, { recursive: true, force: true });"

# Build frontend only
fe:
    cd crates/deepx-tauri && pnpm build

# === Building ===

# Debug build (Tauri)
build: clean-fe
    cd crates/deepx-tauri && pnpm build
    cargo build -p deepx-daemon -p deepx-tauri
    @echo "Binaries: target/debug/deepx-desktop + target/debug/deepx-daemon"

# Release build (clean dist + optimized)
release: clean-fe
    cd crates/deepx-tauri && pnpm build
    cargo build --release -p deepx-daemon -p deepx-tauri
    @echo "Binaries: target/release/deepx-desktop + target/release/deepx-daemon"

# Build installer (.deb / .AppImage / MSI / NSIS)
installer: clean-fe
    cargo build --release -p deepx-daemon
    New-Item -ItemType Directory -Force crates/deepx-tauri/src-tauri/binaries | Out-Null
    Copy-Item target/release/deepx-daemon.exe crates/deepx-tauri/src-tauri/binaries/deepx-daemon-x86_64-pc-windows-msvc.exe -Force
    cd crates/deepx-tauri && pnpm tauri build --config src-tauri/tauri.sidecar.conf.json
    @echo "Installer: crates/deepx-tauri/src-tauri/target/release/bundle/"

# === Check ===

# Type-check frontend only
check-fe:
    cd crates/deepx-tauri && npx tsc --noEmit

# Check Rust only
check-rs:
    cargo check

# Full check
check: check-fe check-rs

# === Clean ===

# Deep clean all artifacts
clean: clean-fe
    cargo clean
    @echo "Done."

# === Testing ===

# Run tool tests
test-tools:
    cargo test -p deepx-tools --release
