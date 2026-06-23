# DeepX build system
# Usage: just [recipe]

set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]

default:
    @just --list

# === Development ===

# Start Tauri dev server (clean dist first)
dev: clean-fe
    cd crates/deepx-tauri && pnpm tauri dev

# Start TUI in dev mode
tui:
    cargo run --release -p deepx-tauri -- --tui

# === Frontend ===

# Clean Vite dist + TypeScript cache (run before build/release/dev)
clean-fe:
    @echo "Cleaning dist + tsbuildinfo..."
    -Remove-Item -Recurse -Force crates/deepx-tauri/dist -ErrorAction SilentlyContinue
    -Remove-Item -Force crates/deepx-tauri/tsconfig.tsbuildinfo -ErrorAction SilentlyContinue

# Build frontend only
fe:
    cd crates/deepx-tauri && pnpm build

# === Building ===

# Debug build (clean dist first)
build: clean-fe
    cd crates/deepx-tauri && pnpm build
    cargo build
    @echo "Binary: target/debug/deepx.exe"

# Release build (clean dist + optimized)
release: clean-fe
    cd crates/deepx-tauri && pnpm build
    cargo build --release
    @echo "Binary: target/release/deepx.exe"

# Build installer (MSI/NSIS)
installer: clean-fe
    cd crates/deepx-tauri && pnpm tauri build
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

# Run TUI tests
test-tui:
    cargo test -p deepx-terminalui

# Run tool tests
test-tools:
    cargo test -p deepx-tools --release

# Run sed engine tests
test-sed:
    cargo test -p deepx-sed --release
