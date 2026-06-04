# DeepX build system
# Run `just` to list all recipes
#
# Binary resolution (dev):
#   tauri-dev mode → find_dsx() searches target/debug/ FIRST, then resources/.
#   So `cargo build -p dsx` updates the running binary without touching resources/.
#
# Binary resolution (pack):
#   tauri-pack bundles resources/dsx into the .deb. It MUST be kept in sync.
#   Always run tauri-pack (not cargo build -p dsx-tauri) to ensure the copy step.

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
# Builds dsx then starts Tauri dev → find_dsx() picks up target/debug/dsx via ancestor walk.
tauri-dev:
    cargo build -p dsx
    cd crates/dsx-tauri && pnpm tauri dev

# Build Tauri desktop app (debug, no installer)
# Copies dsx → resources/ so Tauri bundler embeds the correct binary.
tauri-build:
    cargo build -p dsx
    cp target/debug/dsx crates/dsx-tauri/src-tauri/resources/dsx
    cd crates/dsx-tauri && pnpm build
    cargo build -p dsx-tauri

# Build Tauri .deb package (release)
# Copies dsx → resources/ so Tauri bundler embeds the correct binary.
tauri-pack:
    cargo build --release -p dsx
    cp target/release/dsx crates/dsx-tauri/src-tauri/resources/dsx
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
