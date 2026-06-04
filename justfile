# DeepX build system
# Run `just` to list all recipes

default:
    @just --list

# ─── Dev ───────────────────────────────────────────────

# Check all Rust code (fast)
check:
    cargo check --workspace

# Run all tests
test:
    cargo test --workspace

# Run lints (Rust + frontend)
lint: clippy lint-frontend

clippy:
    cargo clippy --workspace -- -D warnings

lint-frontend:
    cd crates/dsx-tauri && pnpm lint

# ─── Build ─────────────────────────────────────────────

# Build all Rust binaries (debug)
build:
    cargo build -p dsx -p dsx-tui -p dsx-tools

# Build all Rust binaries (release)
build-release:
    cargo build --release -p dsx -p dsx-tui -p dsx-tools

# ─── Tauri ─────────────────────────────────────────────

# Install Tauri frontend dependencies
tauri-deps:
    cd crates/dsx-tauri && pnpm install

# Run Tauri desktop app (dev mode, hot reload)
tauri-dev: _build-dsx-debug
    cd crates/dsx-tauri && pnpm tauri dev

# Build Tauri desktop app (debug, no installer)
tauri-build: _build-dsx-debug
    cargo build -p dsx-tauri

# Build Tauri desktop app (release, produces installer .deb/.dmg)
tauri-release: _build-dsx-release
    pnpm --prefix crates/dsx-tauri build
    cargo build --release -p dsx-tauri

# ─── Run ───────────────────────────────────────────────

# Run TUI (debug)
run: build
    cargo run -p dsx-tui

# Run TUI (release)
run-release: build-release
    cargo run --release -p dsx-tui

# ─── Release ───────────────────────────────────────────

# Full release build (CLI + TUI + Tauri app)
release: build-release tauri-release

# ─── Clean ─────────────────────────────────────────────

# Remove all build artifacts
clean:
    cargo clean
    rm -rf crates/dsx-tauri/node_modules crates/dsx-tauri/dist

# ─── Helpers ───────────────────────────────────────────

_build-dsx-debug:
    cargo build -p dsx

_build-dsx-release:
    cargo build --release -p dsx
