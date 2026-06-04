# DeepX build system
# Run `just` to list all recipes

default:
    @just --list

# ─── Dev ───────────────────────────────────────────────

# Check all Rust code (fast)
check:
    cargo check --workspace

# Type-check frontend TypeScript
check-frontend:
    cd crates/dsx-tauri && npx tsc --noEmit

# Full check (Rust + frontend)
check-all: check check-frontend

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
    cargo build --workspace

# Build all Rust binaries (release)
build-release:
    cargo build --release --workspace

# ─── Install Deps ───────────────────────────────────────

# Install Tauri frontend dependencies
deps:
    cd crates/dsx-tauri && pnpm install

# ─── Tauri ─────────────────────────────────────────────

# Run Tauri desktop app (dev mode, hot reload)
tauri-dev:
    cargo build -p dsx
    cd crates/dsx-tauri && pnpm tauri dev

# Build Tauri desktop app (debug, no installer)
tauri-build:
    cargo build -p dsx
    cd crates/dsx-tauri && pnpm build
    cargo build -p dsx-tauri

# Build Tauri .deb package (release)
tauri-pack:
    cargo build --release -p dsx
    cd crates/dsx-tauri && pnpm build
    cargo build --release -p dsx-tauri
    @echo "→ deb: crates/dsx-tauri/src-tauri/target/release/bundle/deb/"

# ─── Pack ───────────────────────────────────────────────

# Build .deb installer (release)
pack: tauri-pack

# Build .deb + output path
deb: tauri-pack
    @ls -lh crates/dsx-tauri/src-tauri/target/release/bundle/deb/*.deb 2>/dev/null || echo "No .deb found"

# ─── Run ───────────────────────────────────────────────

# Run TUI (debug)
run: build
    cargo run -p dsx-tui

# Run TUI (release)
run-release: build-release
    cargo run --release -p dsx-tui

# ─── Release ───────────────────────────────────────────

# Full release build (all binaries + .deb)
release: build-release pack

# ─── Clean ─────────────────────────────────────────────

# Remove all build artifacts
clean:
    cargo clean
    rm -rf crates/dsx-tauri/node_modules crates/dsx-tauri/dist
    rm -rf crates/dsx-tauri/src-tauri/target
