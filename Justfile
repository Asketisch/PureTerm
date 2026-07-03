# PureTerm build commands

# Build debug
build:
    cargo build --bin pureterm

# Build release
release:
    cargo build --release --bin pureterm

# Build and bundle macOS app
app: release
    bash build.sh app

# Run clippy
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Check formatting
fmt:
    cargo fmt --all -- --check

# Run tests (auto-detect cargo-nextest for parallel execution)
# Install with: cargo install cargo-nextest --locked
test:
    #!/usr/bin/env bash
    set -e
    if command -v cargo-nextest >/dev/null 2>&1; then
        echo "→ using cargo-nextest"
        cargo nextest run --workspace
    else
        echo "→ using cargo test (install cargo-nextest --locked for faster parallel runs)"
        cargo test --workspace
    fi

# Clean build artifacts
clean:
    cargo clean

# Show current config
show-config:
    cargo run -- show-config

# List available themes
list-themes:
    cargo run -- list-themes
