# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Fixed (build / bundling)
- **`build.sh` rewritten to support Win/Linux/macOS multi-arch builds.**
  v0.2 rewrites the macOS bundling so it now correctly:
  - **Default arch follows the host on macOS** (Apple Silicon → arm64 by default;
    the previous version would happily ship x86_64 binaries that triggered
    macOS's "Apple 即将结束对 Intel 芯片 App 的支持" Rosetta warning).
  - **`--universal` flag** builds both `aarch64-apple-darwin` and
    `x86_64-apple-darwin` then runs `lipo -create` to produce a single
    universal2 Mach-O inside `Contents/MacOS/pureterm`. The resulting
    `PureTerm.app` runs natively on both Apple Silicon and Intel Macs without
    any Rosetta warning.
  - **`CFBundleIconFile = PureTerm` is now actually written** into the
    generated `Info.plist` even when the bundle copies from `extra/osx/Info.plist`
    (which previously omitted it, so Finder fell back to a generic icon).
  - **`Contents/Resources/` no longer carries a stray `icon.png`** — only
    `PureTerm.icns` is shipped, so Finder's icon cache and the document-deficon
    slot both read from the right file.
  - **PlistBuddy is invoked via `/usr/libexec/PlistBuddy`** (absolute path);
    on many fresh shells the bare `PlistBuddy` is not on PATH and the previous
    code silently dropped every PlistBuddy call. A `plutil` fallback handles
    CI runners that lack PlistBuddy entirely.
  - **`sccache` no longer hard-errors** when it's not installed on PATH:
    the script now detects its absence and overrides the repo's
    `.cargo/config.toml [build] rustc-wrapper` with a per-invocation
    `--config 'build.rustc-wrapper=""'`. Previously, a fresh checkout on a
    container without sccache would exit 127 on `cargo -vV`.
  - **Cross-platform source cfg gates** added to `crates/pureterm/src/main.rs`
    (modules `macos_url_scheme`, `macos_glass_effect`, `macos_touch_bar`,
    `macos_haptic`) and gated the macOS-only `use objc2::*` imports inside
    `macos_url_scheme.rs`. Without these, the build leaked
    objc2/objc2_foundation symbols into Linux/Windows compilation.
- **`embed-resource` is now declared as an optional
  `[target.'cfg(windows)'.build-dependencies]`** in
  `crates/pureterm-alacritty-runtime/Cargo.toml`. The `build.rs` was already
  referencing `embed_resource::compile(...)` behind a `#[cfg(windows)]` gate;
  declaring the dep means the gate stays green the moment a real
  `windows/alacritty.rc` is dropped in.
- **`crates/pureterm/build.rs` now stamps `VERSION` and `BUILD_TARGET`** into
  the binary on every platform via `cargo:rustc-env`, so packaging-time
  version overrides flow through to `--version` / about boxes. Also records a
  `cargo:rerun-if-changed` hook for a future `windows/pureterm.rc`.

### Added
- **Cross-platform build & bundle pipeline** (`./build.sh` rewritten).
- **Cross-platform build & bundle pipeline** (`./build.sh` rewritten).
  - New `app` / `app-all` / `matrix` / `info` modes. `app` builds +
    bundles for the host OS in the host triple; `app-all` iterates over
    every installed target; `matrix` iterates the full 8-target
    macOS/Linux/Windows matrix and skips targets whose toolchain isn't
    installed (no hard errors).
  - macOS: produces `.app`, `.app.zip`, and (when `hdiutil` is on PATH)
    a UDZO-compressed `.dmg`. Optional `--sign` (`CODESIGN_IDENTITY`) and
    `--notarize` (`APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`).
  - Linux: produces a Freedesktop-compliant staged tree (bin/, share/applications,
    share/metainfo, share/icons, share/doc, INSTALL.md) and packages it
    as `.tar.gz` (always) and `.tar.zst` (when `zstd` is on PATH).
    `--install` copies the tree to `/opt/pureterm-<v>/` and symlinks
    `/usr/local/bin/pureterm`.
  - Windows: produces a portable staged tree and zips it. Optional
    `--sign` (`WINDOWS_CERT_FILE` / `WINDOWS_CERT_PASSWORD` with
    `signtool`). Falls back to PowerShell `Compress-Archive` when `zip`
    isn't on PATH.
  - Pre-flight checks (`cargo metadata`, binary existence, icon
    presence) catch silent build failures.
  - Tool detection is **informational and non-fatal** — the script
    degrades gracefully when hdiutil, codesign, zstd, signtool, etc.
    are missing.
  - See `./build.sh --help` and `./build.sh info` for the full surface.
- **Justfile**: new recipes `app-all`, `matrix`, `info`; `app` and
  `clean` now route through `./build.sh`.
- **`crates/pureterm-alacritty-runtime/Cargo.toml`**: declared
  `embed-resource` as an *optional* `[target.'cfg(windows)'.build-dependencies]`
  entry. The runtime's `build.rs` already gates `embed_resource::compile`
  on a missing `.rc` file; declaring the dep now means the build stays
  green the moment a real `.rc` is dropped in.
- **`crates/pureterm/build.rs`**: now stamps `VERSION` (and optionally
  `BUILD_TARGET`) into the binary on every platform via
  `cargo:rustc-env`, so packaging-time version overrides flow through
  to `--version` / about boxes. Also records a `cargo:rerun-if-changed`
  hook for a future `windows/pureterm.rc`.

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
