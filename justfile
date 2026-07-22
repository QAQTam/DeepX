# DeepX backend build system

default:
    @just --list

# Start the daemon from source.
dev:
    cargo run -p deepx-daemon -- run

# Build the daemon for local Desktop development.
daemon:
    cargo build -p deepx-daemon

# Build the optimized daemon used in releases.
daemon-release:
    cargo build --locked --release -p deepx-daemon

# Produce the local platform asset, release manifest, and SHA256SUMS.
release-assets: daemon-release
    node scripts/package-daemon.mjs --target-id windows-x86_64 --binary target/release/deepx-daemon.exe --output dist
    node scripts/build-release-manifest.mjs --input dist

check:
    cargo check --workspace

test:
    cargo test --workspace

fmt:
    cargo fmt --all --check

clippy:
    cargo clippy --workspace --all-targets

status:
    cargo run -p deepx-daemon -- status

stop:
    cargo run -p deepx-daemon -- stop

clean:
    cargo clean
