# PureTerm build commands
#
# All cross-platform / cross-target work goes through ./build.sh.
# Run `./build.sh --help` for the full option list, or `./build.sh info`
# to see which tools are available on the current host.

# --- Core build -------------------------------------------------------------

# Build debug
build:
    cargo build --bin pureterm

# Build release
release:
    cargo build --release --bin pureterm

# Build & bundle for the host OS in the host triple.
#   macOS  -> dist/PureTerm.app, .app.zip, .dmg
#   Linux  -> dist/pureterm-*/  + .tar.gz
#   Win    -> dist/pureterm-*/  + .zip
app:
    bash build.sh app

# Bundle for the host OS in every installed target triple.
app-all:
    bash build.sh app-all

# Cross-platform build matrix. Skips targets whose toolchain isn't
# installed (no hard error). Cross-OS builds (e.g. .dmg on Linux) are
# skipped by the script — only host-OS bundling runs.
matrix:
    bash build.sh matrix

# Print the detected environment: host OS, installed rustup targets,
# and which bundler tools (hdiutil, codesign, zstd, zip, signtool, …) are
# available. Useful in CI to debug a failing build step.
info:
    bash build.sh info

# --- Test / lint ------------------------------------------------------------

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

# --- Utilities --------------------------------------------------------------

# Clean build artifacts (and the dist/ directory).
clean:
    bash build.sh clean

# Show current config
show-config:
    cargo run -- show-config

# List available themes
list-themes:
    cargo run -- list-themes
