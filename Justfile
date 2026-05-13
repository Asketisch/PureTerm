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

# Run tests
test:
    cargo test --workspace

# Clean build artifacts
clean:
    cargo clean

# Show current config
show-config:
    cargo run -- show-config

# List available themes
list-themes:
    cargo run -- list-themes
