# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Changed
- **Build pipeline aligned with the Reflect-Agent workspace recipe for
  faster test/dev iteration.**
  - `Cargo.toml`: new `[profile.test]` (`debug = 1`, `codegen-units = 256`)
    and `[profile.release-fast]` (`inherits = "release"`, `lto = false`,
    `codegen-units = 16`, `opt-level = 2`, `incremental = true`) sections.
    `[profile.release]` switched from full LTO (`lto = true`) to thin LTO
    (`lto = "thin"`) and from `strip = true` to `strip = "symbols"`,
    trimming link time while keeping binary size within ~5 % of the prior
    build. Use `cargo build --profile release-fast --bin pureterm` for
    iteration-optimised release builds, or override per-invocation with
    `--config 'profile.release.lto=true'` for the old full-LTO behaviour.
  - `.cargo/config.toml`: `rustc-wrapper = "sccache"` is now set
    automatically (cargo falls back to direct rustc invocation when
    sccache is absent, so CI / fresh checkouts still work).
  - `build.sh`: new `test` mode that auto-detects `cargo-nextest` and
    falls back to `cargo test`. Supports `--profile <name>` (default
    `dev`; pass `test` to use the new profile), `--all-features`, and
    `--no-run`. Usage: `./build.sh test` or
    `./build.sh test --profile test --no-run`.
  - `Justfile`: the `test` target now picks `cargo nextest run` when
    available, otherwise `cargo test`.
- **Status bar redesigned.** Segments now render as rounded "chips" tinted
  by `BarGroup` (Context / Runtime / System / Meta) with 8-px inter-group
  spacing. Meta sits at the far right behind the ⌘K button, so cursor
  position (`📍 L:C`) and clock (`🕒 HH:MM:SS`) are visually anchored
  rather than buried in a flat line.
- **Nerd-Font PUA glyphs replaced.** Git branch glyph U+E0A0 → `[br]` text
  prefix; PR pill U+29C9 → `PR#N` text; in-progress op U+21BB → `(rebase)`
  etc. text; hibernated-agent snowflake U+2744 → `💤 (sleep)` text. The bar
  stays readable on machines without Nerd Fonts installed.
- **Top + bottom bar backgrounds** now read from `iced::Theme::Palette`
  (via `status_bar::bar_bg`) instead of hard-coded `rgba(0.12,0.12,0.14,1.0)`.
  Today still resolves to `Theme::Dark` only; the helper path is wired
  for any future light/System theme switch.

### Added
- `BarGroup` enum (`Context` / `Runtime` / `System` / `Meta`) on
  `StatusBarComponent` with a default of `Context` for new components.
- `[statusbar].clock_format` and `[statusbar].clock_show_seconds` config
  keys. `clock_show_seconds = false` strips `%S` / `%s` tokens from the
  format on the fly so a single format reads cleanly in both modes.
- `⌘K` button at the bottom-right of the status bar that opens the command
  palette (re-uses the existing `Message::OpenCommandPalette`).
- `status_bar::chip` module with `chip(label, group, theme)` and
  `chip_with_color(label, group, fg, theme)`, plus `bar_bg` / `bar_border`
  for top/bottom bar fills. All colors derive from `Theme::Palette`.

### Tests
- `components::git_bar_label_*` updated for the `[br]` prefix.
- `agent_indicator_*` updated for the `💤 (sleep)` hibernation suffix.
- New `load_statusbar` tests in `app_config.rs::tests` covering the
  defaults, `clock_format` override, and empty-`components` guard.
- New `status_bar::chip::tests` covering dark-theme construction.

## [0.1.0] - 2026-05-14

### Added
- GPU-accelerated terminal rendering with wgpu
- Ghostty-style tab bar with SDF rounded corners
- Split panes (horizontal and vertical)
- SSH, Serial, Telnet session support
- VTE parser with full CSI/ESC/OSC/DCS/APC/PM sequence handling
- Kitty inline image protocol
- Shell integration via OSC 133 marks
- egui-based settings panel (General, Appearance, Terminal, Profiles, Hotkeys)
- Command palette and profile selector
- Autocomplete and history popup
- Quake mode (dropdown terminal)
- IME support for CJK input
- Accessibility (VoiceOver on macOS)
