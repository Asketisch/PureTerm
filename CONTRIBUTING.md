# Contributing to PureTerm

## Development Setup

```bash
# Build
cargo build

# Run tests
cargo test --workspace

# Run with debug logging
RUST_LOG=debug cargo run

# Check formatting
cargo fmt --all -- --check

# Run clippy
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture

PureTerm uses a 6-crate workspace:

- `pureterm` — Binary entry point, event loop
- `pureterm-core` — Config, theme, hotkeys, plugins
- `pureterm-terminal` — VTE parser, screen grid, grapheme handling
- `pureterm-session` — Session trait + Local/SSH/Serial/Telnet implementations
- `pureterm-ui` — Surface, SplitTree, tabs, terminal panes, settings UI
- `pureterm-gui` — Multi-pass GPU rendering, font system, shaders

See `crates/AGENTS.md` for detailed dependency graph and data flow.

## Pull Request Process

1. Create a feature branch from `main`
2. Make your changes with clear commit messages
3. Ensure `cargo fmt`, `cargo clippy`, and `cargo test` pass
4. Submit a PR with a description of the changes

## Coding Conventions

- Follow standard Rust formatting (`cargo fmt`)
- Resolve all clippy warnings
- Use Chinese comments to match existing codebase style
- UI elements must follow the rounded corner design language (see CLAUDE.md)
