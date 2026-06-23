# DeepX build system
# Usage: just <recipe>

set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]

default:
    @just --list

# === Development ===

# Start Tauri dev server (hot-reload frontend + Rust)
dev:
    cd crates/deepx-tauri && pnpm tauri dev

# Start TUI in dev mode
tui:
    cargo run --release -p deepx-tauri -- --tui

# Type-check frontend only
check-fe:
    cd crates/deepx-tauri && pnpm tsc --noEmit

# Check Rust compilation
check-rs:
    cargo check

# === Building ===

# Build release binary (deepx.exe with all modes)
build:
    cd crates/deepx-tauri && pnpm build
    cargo build --release -p deepx-tauri
    @echo "Binary: target/release/deepx.exe"

# Build installer (MSI/NSIS)
release:
    cd crates/deepx-tauri && pnpm tauri build
    @echo "Installer: crates/deepx-tauri/src-tauri/target/release/bundle/"

# Clean all build artifacts
clean:
    cargo clean
    Remove-Item -Recurse -Force crates/deepx-tauri/dist -ErrorAction SilentlyContinue; Remove-Item -Recurse -Force crates/deepx-tauri/node_modules/.cache -ErrorAction SilentlyContinue

# === Testing ===

# Run TUI tests
test-tui:
    cargo test -p deepx-tui

# Run tool tests
test-tools:
    cargo test -p deepx-tools --release

# Run sed engine tests
test-sed:
    cargo test -p deepx-sed --release
