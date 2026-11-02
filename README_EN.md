<div align="center">

[![CI](https://github.com/Asketisch/PureTerm/actions/workflows/ci.yml/badge.svg)](https://github.com/Asketisch/PureTerm/actions/workflows/ci.yml)
[![Release](https://github.com/Asketisch/PureTerm/actions/workflows/release.yml/badge.svg)](https://github.com/Asketisch/PureTerm/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)

[简体中文](README.md) · **English**

</div>

# PureTerm

> **Close the window — your sessions survive.**
> A GPU-accelerated terminal built on the Alacritty core and Iced 0.14, written in pure Rust. A standalone daemon owns the PTY: quit, crash, or kill the GUI — builds, deployments and SSH sessions keep running, and every tab, split, layout and scrollback is restored when you return. **No tmux required.**
> On top of that: an SSH client with an encrypted credential vault, a JSON-RPC control plane covering the entire UI, and native AI-agent support — policy hooks, approvals and session resume for Claude Code & friends. Everything runs locally, MIT licensed.

---

## Table of Contents

- [Why PureTerm](#why-pureterm)
- [Highlights](#highlights)
- [Install Prebuilt Binaries](#install-prebuilt-binaries)
- [Build from Source](#build-from-source)
- [Feature Deep-Dives](#feature-deep-dives)
  - [Sessions That Survive: Closing a Tab ≠ Quitting the App](#sessions-that-survive-closing-a-tab--quitting-the-app)
  - [SSH as a First-Class Citizen](#ssh-as-a-first-class-citizen)
  - [AI-Agent Native](#ai-agent-native)
  - [A Programmable Control Plane: Socket API + CLI](#a-programmable-control-plane-socket-api--cli)
  - [Configuration: Alacritty-Compatible + Hot Reload](#configuration-alacritty-compatible--hot-reload)
  - [The Fundamentals](#the-fundamentals)
- [Keybindings Cheat Sheet](#keybindings-cheat-sheet)
- [Architecture](#architecture)
- [Build & Packaging](#build--packaging)
- [Roadmap](#roadmap)
- [Contributing](#contributing)

---

## Why PureTerm

Three old problems for terminal users — three new answers from PureTerm:

**1. "Close the terminal, lose everything."**

A half-finished build, a live SSH session — gone with the window. tmux saves you, but every session starts with `tmux new`, resuming needs `attach`, multiple clients demand prefix keys — a real habit tax. PureTerm pushes this down to the PTY layer: a standalone daemon, `puretermd`, holds the PTY master fd, and the GUI is just one of its clients. Quit the GUI, kill the process, even crash — sessions keep running and reconnect automatically. **No tmux, nothing new to learn.**

**2. "Your SSH toolchain is a DIY kit."**

Hosts scattered across `~/.ssh/config`, passwords in your head (or worse, in plaintext), and a full redo after every disconnect. PureTerm treats SSH as a first-class citizen: profiles, an AES-256-GCM / Argon2id encrypted credential vault, ControlMaster connection sharing across tabs, and automatic reconnect with exponential backoff — scrollback intact. `⌘⇧S` and you're there.

**3. "In 2026, a terminal must serve AI agents."**

Claude Code and friends run hundreds of commands in your terminal every day — who decides what's allowed, blocked, and audited? Most terminals leave you to glue scripts together. PureTerm answers natively: a hooks policy middleware (regex-deny `rm -rf`, external approval scripts), one-command onboarding via `pureterm agents install claude-code`, live agent status in the status bar, automatic `claude --resume` after a restart, and a local audit log for every decision. AI-centric terminals are everywhere; ones that keep agent policy **local and open to audit** are rare — PureTerm uploads nothing.

### How It Compares

| Capability | **PureTerm** | Alacritty | iTerm2 | Kitty | WezTerm | tmux |
|---|---|---|---|---|---|---|
| GPU rendering | ✅ wgpu (Metal / Vulkan / DX12) | ✅ | ✅ Metal | ✅ | ✅ | ➖ |
| Tabs + nested splits | ✅ | ❌ | ✅ | ✅ | ✅ | ✅ |
| **Sessions survive** GUI close / crash | ✅ daemon owns the PTY, zero habit tax | ❌ | ⚠️ processes die with the GUI | ❌ | ❌ | ✅ (if you wrap your work in tmux) |
| Restore tabs / splits / layout / scrollback on relaunch | ✅ local + SSH | ❌ | ⚠️ window layout only | ⚠️ predefined sessions, processes don't survive | ❌ (community plugins) | ✅ |
| SSH: encrypted vault + ControlMaster + auto-reconnect | ✅ built-in | ❌ | ⚠️ keychain passwords | ❌ | ❌ | ❌ |
| AI agents: policy hooks / one-step adapter / resume / audit | ✅ native | ❌ | ❌ | ❌ | ❌ | ❌ |
| Programmatic control (JSON-RPC + CLI) | ✅ 31 methods / 8 namespaces | ❌ | ⚠️ Python API | ⚠️ remote control | ⚠️ Lua plugin API | ✅ |
| GUI settings (format-preserving write-back) | ✅ dedicated process | ❌ | ✅ | ❌ | ❌ | ❌ |
| Alacritty TOML config compatibility | ✅ | ✅ (that's it) | ➖ | ➖ | ➖ | ➖ |
| macOS / Linux / Windows | ✅ | ✅ | macOS only | macOS / Linux | ✅ | ✅ |

> Alacritty is a kernel-only philosophy (no tabs, no splits, no settings UI). PureTerm uses it as the kernel and completes the picture with a full GUI layer — your existing `alacritty.toml` migrates as-is.

---

## Highlights

- 🔒 **Sessions that survive** — the `puretermd` daemon holds PTY master fds: GUI close / crash never interrupts work; relaunch restores tab order, split trees, window sizes and scrollback, and SSH reconnects without re-auth
- 🛡 **First-class SSH** — ControlMaster connection sharing across tabs, `~/.ssh/config` works as-is, AES-256-GCM + Argon2id encrypted credentials, exponential-backoff auto-reconnect with scrollback preserved
- 🤖 **AI-agent native** — hooks policy middleware (block `rm -rf` and friends), `pureterm agents install claude-code` onboarding, status-bar agent indicators, `claude --resume` cold restore, local audit logs
- ⌨️ **Programmable control plane** — JSON-RPC 2.0 over a Unix domain socket with 31 methods; structured CLI like `pureterm tab new` / `pane split`, friendly to scripts and automation
- 🎨 **GPU rendering + Alacritty-compatible** — custom wgpu shaders (Metal / Vulkan / DX12); a standard `alacritty.toml` works as-is, and a dedicated settings process hot-reloads every change
- 🧩 **And more** — command palette (`⌘⇧P` fuzzy search), the full split toolkit (equalize / swap / zoom / move across tabs), Vi copy mode, 9 smart-selection rules, 6 keymap presets, custom command snippets, status bar (CWD / Git / SSH / agent / cursor)

---

## Install Prebuilt Binaries

Pushing a `v*` tag makes CI build and publish artifacts for every platform on [GitHub Releases](../../releases):

| Platform | Artifact | Install |
|---|---|---|
| macOS Apple Silicon | `PureTerm-<v>-arm64.dmg` (or `.app.zip`) | Open the .dmg, drag to Applications |
| macOS Intel | `PureTerm-<v>-x64.dmg` (or `.app.zip`) | Same |
| Linux x86_64 | `pureterm-<v>-x64-gnu.tar.gz` | Extract; the tree contains `bin/` + `share/` — drop it into `/usr/local` |
| Linux aarch64 | `pureterm-<v>-arm64-gnu.tar.gz` | Same |
| Windows x86_64 | `pureterm-<v>-x64.zip` | Extract, add `bin` to `PATH` |

Each archive ships three binaries: `pureterm` (main app), `pureterm-settings` (settings UI) and `puretermd` (session daemon) — keep them in the same directory. `SHA256SUMS` checksums accompany every release.

Note: Linux binaries are linked on Ubuntu 22.04 (x86_64) / 24.04 (aarch64) and require a reasonably recent glibc; on older distros, build from source.

## Build from Source

```bash
git clone https://github.com/Asketisch/PureTerm && cd PureTerm
cargo build --release
./target/release/pureterm
```

Config file: `~/.config/pureterm/alacritty.toml` (falls back to `~/.config/alacritty/alacritty.toml`).
For cross-platform / cross-target packaging see [Build & Packaging](#build--packaging).

---

## Feature Deep-Dives

### Sessions That Survive: Closing a Tab ≠ Quitting the App

`puretermd` is a singleton daemon (flock + setsid, detached from the GUI process group). Its pump threads own each session's PTY master fd, backed by a 1 MiB per-session ring buffer. The GUI is merely one client — several clients can attach to the same session at once.

What gets restored: on GUI relaunch — tab order, titles, split trees (direction + ratio), window sizes, active tab, and the visible scrollback region (rebuilt by replaying the ring buffer). Orphan sessions (alive in the daemon, missing from the snapshot) come back as extra tabs. SSH sessions reconnect to their live `ssh -tt` children **without re-authentication**.

Closing semantics are deliberate:

| Action | Session outcome | Next launch |
|---|---|---|
| `⌘W` closes a tab / pane | exit → SIGHUP; the daemon session ends | **Not** restored |
| Quit the app (`⌘Q` / close window / kill) | All sessions kept, still running | Full layout + scrollback restored |

In short: *closing a tab discards that session; quitting just means you left.* Layout snapshots are written atomically with debouncing; if the daemon is unavailable, PureTerm falls back to a plain single-tab launch with zero regression.

### SSH as a First-Class Citizen

- **Profiles** declared in `alacritty.toml`; connect with `⌘⇧S` or by typing `ssh` in the command palette. Host aliases from `~/.ssh/config` work out of the box
- **ControlMaster** socket reuse — multiple tabs share one SSH connection
- **Encrypted credential vault**: AES-256-GCM + Argon2id; the master key lives in `Zeroizing` and is wiped on exit
- **Auto-reconnect**: exponential backoff (3→60s × 8) on network loss, scrollback preserved; the status bar shows `SSH` / `SSH ↻ n` / `SSH ✗`; an explicit `pureterm ssh disconnect` never triggers reconnection

```toml
[[ssh.profiles]]
name = "Production"
host = "192.168.1.100"
user = "admin"

[[ssh.profiles]]
name = "Bastion"
host = "bastion"
key_path = "~/.ssh/bastion_key"
```

### AI-Agent Native

PureTerm acts as a **hooks policy middleware** for Claude Code and other agents: before every tool call, the agent hands the event to `pureterm hook <event>`; PureTerm evaluates your rules and answers with an allow/deny decision in the Claude Code hooks protocol. Standalone by design — zero IPC round-trips, each tool call just reads local config.

```toml
[[agent.hook]]
event = "PreToolUse"
matcher = "Bash"                # regex over the tool name (Pre/PostToolUse only)
deny_pattern = "\\brm\\s+-rf"   # match → deny
deny_reason = "rm -rf blocked by policy"

[[agent.hook]]
event = "PostToolUse"
matcher = "Write|Edit"
command = "/abs/path/lint.sh"   # external script: event JSON on stdin, stdout/exit code = decision
timeout = 30
```

```bash
pureterm agents install claude-code        # registers into ~/.claude/settings.json (locked, atomic, idempotent, preserves existing content)
pureterm agents install claude-code --dry-run
pureterm agents detect                     # scan for agent sessions running on this machine
pureterm feed list                         # approval / decision audit log (feed.jsonl)
```

- Events aligned with the official protocol: `PreToolUse` / `PostToolUse` / `UserPromptSubmit` / `Notification` / `Stop` / `SubagentStop` / `PreCompact` / `SessionStart` / `SessionEnd`
- **Cold restore**: with `[[vault.agents]]` configured, agent session ids persist; relaunching PureTerm prompts for / runs `claude --resume` — process scanning can even discover agents started outside PureTerm
- **Status-bar agent indicators**: `⚡ kind` (active) / `❄ kind` (hibernating) / `⏳ N` (N pending approvals)
- Everything executes locally: policies evaluated locally, audits written locally, credentials encrypted locally — no cloud, MIT, fully auditable

### A Programmable Control Plane: Socket API + CLI

While running, PureTerm exposes **JSON-RPC 2.0** on a Unix domain socket: 8 namespaces (`system` / `window` / `tab` / `pane` / `surface` / `config` / `ssh` / `debug`), 31 methods, reads and writes alike — writes drive the GUI for real (`tab.new` opens a new tab). The CLI discovers the socket automatically:

```bash
pureterm tab new                   # new tab
pureterm tab switch --index 1
pureterm pane split --direction h  # horizontal split
pureterm pane zoom
pureterm ssh list-profiles
pureterm config reload             # hot-reload config
pureterm system methods            # list every method

pureterm --json tab list           # compact JSON output
pureterm --socket /tmp/x.sock system ping                   # explicit socket
pureterm msg pane.split '{"direction":"horizontal"}'        # raw JSON-RPC escape hatch (any method)
```

### Configuration: Alacritty-Compatible + Hot Reload

- Config lives at `~/.config/pureterm/alacritty.toml` and is **fully compatible with the standard Alacritty TOML schema** — migrate your existing config as-is; the fallback path `~/.config/alacritty/alacritty.toml` works too
- `⌘,` opens a **dedicated settings process**, `pureterm-settings` (six pages: General / Appearance / Colors / Terminal / SSH Profiles / Keyboard Shortcuts); changes are written back via `toml_edit` with format preserved and atomically, and the main process hot-reloads on file change
- Keybindings: 31+ actions customizable in TOML, 6 preset templates (vim / tmux / macos / vscode / agent-triage), CLI management (`pureterm shortcuts preset vim` / `reset`), hot reload on save
- **Custom commands**: turn shell snippets into "bind a key + searchable in the palette + one keystroke", executed in the current pane with visible output:

  ```toml
  [[commands]]
  id = "deploy.staging"
  title = "Deploy to Staging"
  key = "Cmd+Shift+D"          # optional; palette-only if omitted
  command = "ssh staging 'cd app && ./deploy.sh'"
  ```

### The Fundamentals

- **Rendering**: Iced 0.14 + custom wgpu shaders; Metal / Vulkan / DX12 auto-selected; LRU glyph atlas + dirty regions; text anti-blur handling for low-DPI displays
- **Full split toolkit**: nested horizontal / vertical splits, drag to resize, equalize `⌘=`, swap `⌘⇧X`, temporary zoom `⌘⇧Enter`, DFS focus cycling `⌘]` / `⌘[`, move to new tab / window — from keyboard, context menu and command palette alike
- **Vi copy mode**: `Ctrl+Shift+Space` to enter, hjkl navigation, v/V/Ctrl+v selections, y to yank, `/` `?` search — with GPU-rendered vi cursor and search highlights
- **Smart selection, 9 rules**: URL / SSH URL / email / quoted strings / file paths / C++ namespaces / dotted includes / Git SHA / IPv4
- **Command palette**: `⌘⇧P`, nucleo fuzzy search, dual mode for Actions / SSH Profiles
- **Status bar**: working directory (OSC 7) · Git branch · SSH indicator · foreground process · agent indicator · cursor position
- Full VTE parsing (CSI / ESC / OSC / DCS / APC / PM) and IME candidate support

---

## Keybindings Cheat Sheet

macOS shown; the full mapping is viewable and editable under Settings → Keyboard Shortcuts.

| Action | Keys | Notes |
|---|---|---|
| New tab | `⌘T` | inherits working directory |
| Close tab / pane | `⌘W` | SSH tabs close gracefully (exit + SIGHUP fallback) |
| Previous / next tab | `⌘⇧[` / `⌘⇧]` | |
| Jump to tab N | `⌘1` … `⌘9` | |
| Horizontal / vertical split | `⌘D` / `⌘⇧D` | |
| Equalize all splits | `⌘=` | recursive |
| Swap panes | `⌘⇧X` | |
| Temporarily zoom pane | `⌘⇧Enter` | press again to restore |
| Focus next / previous pane | `⌘]` / `⌘[` | DFS pre-order cycling |
| Move pane to new tab / window | `⌘⇧T` / `⌘⇧N` | |
| Command palette | `⌘⇧P` | nucleo fuzzy search |
| Connect SSH | `⌘⇧S` | SSH profile picker |
| Settings | `⌘,` | opens the dedicated settings process |
| Vi copy mode | `Ctrl+Shift+Space` | hjkl move, v select, y yank, `/` search |
| Search forward / backward | `⌘F` / `⌘⇧F` | |

---

## Architecture

PureTerm is a Rust workspace:

| Crate | Purpose |
|---|---|
| `pureterm` | Main binary, an Iced 0.14 daemon — tabs / splits / SSH / command palette / IPC / agent hooks all live on the main path |
| `pureterm-settings` | Dedicated settings UI process (Iced daemon), launched with `⌘,` |
| `pureterm-daemon` | The `puretermd` session daemon — owns PTY master fds so sessions survive the GUI |
| `pureterm-ssh` | SSH backend: ControlMaster reuse, encrypted credential vault, auto-reconnect |
| `alacritty_terminal` (+ `alacritty_config*`) | Vendored upstream — terminal kernel / PTY / grid / VTE parsing |
| Others (`pureterm-jsonrpc` / `pureterm-lsp` / `pureterm-codeindex` / …) | IPC protocol, LSP, code indexing and supporting crates |

Maintenance rules: new features land on the main path in `crates/pureterm/src/`; the vendored Alacritty sources are never touched for formatting or refactors (to ease upstream syncs) — any necessary change carries a comment explaining why. See [`crates/AGENTS.md`](crates/AGENTS.md).

---

## Build & Packaging

All cross-platform / cross-target work goes through `./build.sh` — one script that produces
macOS `.app` + `.dmg`, Linux `.tar.gz`, and Windows `.zip`.

### Quick Commands

```bash
./build.sh --help          # all options
./build.sh info            # detected host, installed rustup targets, available tools
./build.sh release         # release binaries only
./build.sh app             # package for host OS + host triple
./build.sh app-all         # package for every installed triple of the host OS
./build.sh matrix          # cross-platform matrix packaging (skips missing targets)
./build.sh test            # run tests (nextest → cargo test)
./build.sh clean           # remove target/ and dist/
```

### Artifacts per Platform

| Host OS | Mode | Artifacts (in dist/) |
|---|---|---|
| macOS | `app` | `PureTerm.app/` · `PureTerm-<v>-arm64.app.zip` · `PureTerm-<v>-arm64.dmg` |
| macOS | `app --universal` | `PureTerm.app/` (universal2 binaries) · `PureTerm-<v>-universal.app.zip` · `PureTerm-<v>-universal.dmg` |
| Linux | `app` | `pureterm-<v>-<arch>-<env>/` (staged tree) · `pureterm-<v>-<arch>-<env>.tar.gz` |
| Windows | `app` | `pureterm-<v>-<arch>/` · `pureterm-<v>-<arch>.zip` |

### macOS Universal2 Binaries (`--universal`)

```bash
./build.sh --universal app
# or
./build.sh app --bundle all --universal
```

- Builds both `aarch64-apple-darwin` and `x86_64-apple-darwin`, then merges them into a single universal2 Mach-O with `lipo`, running natively on Apple Silicon and Intel Macs.
- Prerequisites: both rustup targets installed, and the host must be macOS.
- Artifacts carry a `-universal` suffix to distinguish them from single-arch builds.

### Target Selection

```bash
# single target
./build.sh --target aarch64-apple-darwin app
./build.sh --target x86_64-pc-windows-msvc app

# multiple targets (comma-separated)
./build.sh --targets aarch64-apple-darwin,x86_64-apple-darwin app

# filter by architecture (expands to all matching triples)
./build.sh --arch x86_64 app-all
./build.sh --arch aarch64 matrix
```

### Tool Availability (`./build.sh info`)

The packaging script degrades gracefully when a tool is missing — no `hdiutil` never blocks a build:

| Tool | Purpose | Behavior when missing |
|---|---|---|
| `sccache` | compile cache | silent fallback to plain rustc |
| `cargo-nextest` | parallel tests | falls back to `cargo test` |
| `mold` / `lld` | faster Linux linking | falls back to the system ld |
| `hdiutil` | macOS .dmg | skip .dmg, still produce .app.zip |
| `codesign` + `CODESIGN_IDENTITY` | macOS code signing | skip signing |
| `xcrun notarytool` + `APPLE_ID` etc. | macOS notarization | skip notarization |
| `zstd` | Linux .tar.zst | skip the zst format |
| `zip` / `powershell` | Windows .zip | error if neither exists |
| `signtool` + cert env vars | Windows code signing | skip signing |

### Cross-Platform Build Matrix

`./build.sh matrix` walks all 8 targets below, skipping rustup targets missing on the host and impossible cross-compiles; `--auto-install-targets` installs missing targets automatically:

| Host | x86_64 | aarch64 |
|---|---|---|
| macOS | `x86_64-apple-darwin` | `aarch64-apple-darwin` |
| Linux (gnu) | `x86_64-unknown-linux-gnu` | `aarch64-unknown-linux-gnu` |
| Linux (musl, fully static) | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-musl` |
| Windows (MSVC) | `x86_64-pc-windows-msvc` | `aarch64-pc-windows-msvc` |

### CI

- [`ci.yml`](.github/workflows/ci.yml) — three-platform build + test + clippy (push / PR)
- [`release.yml`](.github/workflows/release.yml) — pushing a `v*` tag builds 5 targets and publishes a GitHub Release with `SHA256SUMS`; the tag must match the version in `crates/pureterm/Cargo.toml`

### Packaging Formats Not Yet Implemented

`.deb` / `.rpm` / `.AppImage` / `.msi` / NSIS are not provided yet (v1 focuses on portable `.tar.gz` / `.zip` / `.dmg`). Open an issue if you need one.

---

## Roadmap

**Done** (2026 Q2): persistent session daemon + cross-restart restore, first-class SSH (backend / credential vault / settings panel / auto-reconnect), Socket API v2 + structured CLI, the full split toolkit, Vi copy mode, status bar, command palette, agent hooks framework + one-step Claude Code adapter, theme system and cross-platform titlebar integration.

**In progress**: GUI end-to-end hardening of the agent experience loop (approval panel / hibernation / cold restore / external agent discovery); more package formats (`.deb` / `.rpm` / `.msi`).

**Planned**: browser panel (WebView + SOCKS5 tunnel), dual sidebars + workspace abstraction, more agent adapters (Codex / OpenCode etc.).

The full roadmap lives in [`docs/todo.md`](docs/todo.md) (todo), [`docs/progressing.md`](docs/progressing.md) (in progress) and [`docs/done.md`](docs/done.md) (done).

---

## Contributing

Issues and PRs are welcome — see [`CONTRIBUTING.md`](CONTRIBUTING.md).

Project navigation for (AI-assisted) contributors: [`AGENTS.md`](AGENTS.md), [`CLAUDE.md`](CLAUDE.md), [`crates/AGENTS.md`](crates/AGENTS.md).

Three hard rules:

1. Never reformat or refactor the vendored Alacritty sources — it eases upstream syncs
2. New features go on the main path in `crates/pureterm/src/`
3. Any necessary change to vendored code carries a comment explaining why

## License & Acknowledgements

MIT license. The terminal kernel is based on [Alacritty](https://github.com/alacritty/alacritty) (vendored, Apache-2.0 / MIT); the GUI on [iced](https://github.com/iced-rs/iced); rendering on [wgpu](https://github.com/gfx-rs/wgpu); fuzzy matching on [nucleo](https://github.com/helix-editor/nucleo). Many thanks to these excellent open-source projects.
