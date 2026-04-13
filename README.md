# PureTerm

A GPU-accelerated terminal emulator written in pure Rust.

## Features

- **Multi-tab terminal** with nested splits
- **Multiple session types**: Local shell, SSH, Serial, Telnet
- **GPU-accelerated rendering** via wgpu
- **Cross-platform**: macOS, Linux, Windows
- **Themeable** with custom color schemes
- **Configurable keybindings**
- **Plugin system** for extensibility

## Quick Start

```bash
cargo build --release
./target/release/pureterm
```

## Architecture

PureTerm is organized as a workspace with 6 crates:

| Crate | Description |
|-------|-------------|
| `pureterm` | Binary entry point, event loop |
| `pureterm-core` | Config, themes, keybindings, plugins |
| `pureterm-terminal` | VTE parser, screen grid |
| `pureterm-session` | Session trait + Local/SSH/Serial/Telnet |
| `pureterm-ui` | egui components (tabs, split panes) |
| `pureterm-gui` | wgpu rendering, font atlas, shaders |

```
User Input → TerminalPane::handle_key → Session::write
Session Output → TerminalPane::drain_output → Terminal::write → VteParser → Grid
Rendering → ScreenSnapshot → TerminalRenderer::render → wgpu GPU
```

## Tech Stack

- **Rendering**: wgpu + egui
- **Terminal parsing**: vte
- **Window/Events**: winit
- **Local shell**: portable-pty
- **SSH**: russh
- **Async**: tokio

## Project Structure

```
pureterm/
├── crates/
│   ├── pureterm/           # Binary
│   ├── pureterm-core/      # Config, themes, keybindings
│   ├── pureterm-terminal/  # Terminal emulation
│   ├── pureterm-session/   # Session management
│   ├── pureterm-ui/        # UI components
│   └── pureterm-gui/      # GPU rendering
├── build.sh
├── Cargo.toml
└── README.md
```

## License

MIT
