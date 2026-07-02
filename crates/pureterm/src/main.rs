mod automation;
mod script_cli;
mod macos_url_scheme;
mod agent_adapters;
mod agent_detect;
#[cfg(all(test, unix))]
mod agent_e2e;
/// GAP-6.6: join-pane Arc-follows invariant (headless, no PTY) — the
/// cross-module seam `apply_attach_pane` drives on the split tree + pane map.
#[cfg(all(test, unix))]
mod join_pane_e2e;
/// GAP-2.1: Workspace 切换「不丢 Surface 状态」不变量(headless,无 PTY)。
#[cfg(all(test, unix))]
mod workspace_e2e;
/// GAP-2.1: detached 键盘路由「焦点指针决定按键落点」不变量(headless,无 GUI)。
#[cfg(all(test, unix))]
mod keyboard_routing_e2e;
/// GAP-3.5: Browser 地址栏 ⌘⇧L/⌘L — spawn-web 插入序列 + confirm load_url
/// 往返(headless,无 PTY/GUI)。
#[cfg(all(test, unix))]
mod browser_address_bar_e2e;
mod agent_hooks;
mod agent_sessions;
/// GAP-01: built-in AI Chat Panel — LLM provider abstraction (Anthropic +
/// OpenAI-compatible), SSE streaming, Markdown rendering, dockable panel.
mod ai_chat;
/// GAP-02: Error Recovery — LLM fix suggestions on command failure.
mod error_recovery;
/// GAP-04: Computer Use — desktop screenshot + input stubs.
mod computer_use;
/// GAP-08: Natural language shell commands (`# description`).
mod natural_language;
/// GAP-07: Soul System — personality files for AI Chat.
mod soul;
/// GAP-4.2: Smart Selection Hover Preview — tooltip + semantic actions.
mod hover_preview;
/// GAP-33: Port Scanner — listening ports sidebar tab.
mod port_scanner;
/// GAP-43: Remote file browse — sidebar Files tab.
mod remote_files;
/// GAP-06: Ambient Agents local task queue MVP.
mod ambient_agents;
/// GAP-13: Broadcast keyboard input to multiple panes.
mod broadcast_input;
/// GAP-18: BiDi text reorder helper.
mod bidi;
/// GAP-26: Workspace sidebar groups.
mod workspace_groups;
/// GAP-30: Toolbelt sidebar panel.
mod toolbelt;
/// GAP-31: Quake-style dropdown terminal.
mod quake_dock;
/// GAP-37: Password manager integration for SSH.
mod password_manager;
/// GAP-39: Serial & Telnet session types.
mod serial_telnet;
/// GAP-42: SSH login scripts.
mod login_scripts;
mod app_config;
/// GAP-3.5: 内置 Browser 面板(WebView)— pane 的第二种内容载体。
mod browser;
mod cli;
mod codeindex;
mod config_watcher;
/// GAP-6.7: unified event schema (Category + AppEvent).
mod events;
/// GAP-6.7: in-process event hub (broadcast + JSONL dual-write).
mod events_bus;
/// GAP-6.7: pure file I/O for the event log (append/rotate/read/tail).
mod events_log;
/// GAP-32: GitHub PR status via the `gh` CLI (reuses the user's `gh auth`).
mod gh_pr;
#[cfg(unix)]
mod feed_client;
/// GAP-7.6: read-only feed audit log viewer (`pureterm feed list`).
mod feed_log;
#[cfg(unix)]
mod feed_panel;
#[cfg(unix)]
mod hibernate_panel;
/// GAP-7.6: detect-resume prompt panel state machine (GUI-side).
#[cfg(unix)]
mod detect_panel;
#[cfg(unix)]
mod daemon_client;
#[cfg(unix)]
mod daemon_cli;
#[cfg(unix)]
mod daemon_pty;
mod iced_app;
mod ime_widget;
mod ipc;
/// GAP-7.10: MCP server (`pureterm mcp serve`) — exposes the terminal surface
/// to AI agents as native MCP tools. rmcp is feature-gated (`mcp-server`);
/// the roster/whitelist/config pieces stay compiled without the feature.
mod mcp;
mod keymap;
/// GAP-35: LSP client — rust-analyzer stdio + diagnostics.
mod lsp;
mod pane_exit;
mod search;
mod session_restore;
mod split_tree;
/// GAP-19: Dual Sidebar — left (Workspace + Git) / right (Find / Feed / Git).
mod sidebar;
/// GAP-3.2: Programmable sidebar DSL (TOML scripts + EventBus).
mod sidebar_dsl;
/// GAP-11: Instant Replay — events.jsonl timeline export + scrub viewer.
mod instant_replay;
/// GAP-12: Annotations & Marks — OSC 133 grid marks + line notes.
mod annotations;
/// GAP-25: Multi-window cross-process IPC (workspace.switch fan-out).
mod multi_window_ipc;
mod status_bar;
mod surface_snapshot;
#[cfg(target_os = "macos")]
mod macos_menu;
#[cfg(target_os = "macos")]
mod macos_reopen;
/// GAP-09 / GAP-21: Dock badge (unread + Feed pending) via NSDockTile.
#[cfg(target_os = "macos")]
mod macos_dock_badge;
/// GAP-21: Dock right-click menu — recent workspaces via NSMenu.
#[cfg(target_os = "macos")]
mod macos_dock_menu;
/// GAP-09: macOS desktop notifications (UNUserNotificationCenter toasts).
#[cfg(target_os = "macos")]
mod macos_notifications;
#[cfg(target_os = "macos")]
mod macos_eyedropper;
#[cfg(unix)]
mod ssh;
#[cfg(unix)]
mod ssh_cli;
/// GAP-5.2: `pureterm ssh tunnel …` — russh SOCKS5 tunnel (daemon-direct).
#[cfg(unix)]
mod tunnel_cli;
/// GAP-7.7: custom resume bindings (`pureterm surface resume ...`).
#[cfg(unix)]
mod resume_bindings;
/// GAP-6.6: `pureterm tmux …` — tmux-compatible command subset.
mod tmux_cli;
/// T-core-tmux-cc: tmux control mode (-CC) backend — envelope parser +
/// PTY skeleton + Surface variant + mirror.
mod tmux_cc;
/// T-core-windows-conpty: cross-platform local-socket abstraction.
mod local_socket;
/// GAP-32: Git overlay panel state + quick-action enum (view lives in iced_app).
mod git_panel;
/// GAP-09: in-memory notification store (history + unread + attention + bell throttle).
mod notification_store;
/// GAP-09: Notifications overlay panel state (view lives in iced_app).
mod notification_panel;
/// GAP-15: per-pane shell-integration state (OSC 133 A/B/C/D state machine).
mod shell_state;
/// GAP-51: block-based output scaffold (data model + feature flag).
mod output_blocks;
/// GAP-10: pattern-match triggers (regex on output → highlight/notify/send_text/exec).
mod triggers;
/// GAP-15: AI-facing shell context builder (MCP get_shell_context + future AI panel).
mod shell_context;
mod terminal_context;
mod terminal_shader;
mod vi_mode_handler;
/// GAP-05: Voice input (push-to-talk scaffold).
mod voice_input;
/// GAP-17: Sixel / Kitty graphics protocol scaffold.
mod terminal_graphics;
/// GAP-20: macOS glass / vibrancy effect.
mod macos_glass_effect;
/// GAP-22: MacBook Touch Bar scaffold.
mod macos_touch_bar;
/// GAP-23: Visual tab bar config.
mod visual_tab_bar;
/// GAP-28: Custom window border.
mod window_border;
/// GAP-29: Environment badge overlay.
mod badge_overlay;
/// GAP-34: File preview (image/PDF/text).
mod file_preview;
/// GAP-38: SCP/SFTP file transfer.
mod file_transfer;
/// GAP-44: Shared sessions (stub).
mod shared_sessions;
/// GAP-45: Auto-update check scaffold.
mod auto_update;
/// GAP-49: Plugin system (stub).
mod plugin_system;
/// GAP-50: Cloud sync (local bundle MVP).
mod cloud_sync;
/// GAP-52: Parameterized workflows.
mod workflows;
/// GAP-53: Interactive notebooks.
mod notebooks;
/// GAP-54: Background image config.
mod background_image;
/// GAP-55: Window transparency + blur.
mod window_effects;
/// GAP-56: Focus-follows-mouse.
mod focus_follows_mouse;
/// GAP-57: MacBook haptic feedback.
mod macos_haptic;
/// GAP-58: Extended theme overrides.
mod custom_theme;
/// GAP-59: Emoji rendering channel.
mod emoji_rendering;
/// GAP-60: Pane screenshot / export.
mod pane_capture;
/// GAP-61: Print support.
mod print_support;
/// GAP-4.6: VoiceOver / accessibility announcements.
mod accessibility;
/// P4 GAP CLI helpers (plugin, sync, share, …).
mod p4_cli;
/// P4 GAP runtime bundle for iced_app.
mod p4_runtime;

fn main() -> iced::Result {
    env_logger::init();

    // GAP-6.2: structured CLI command groups (clap derive). Bare `pureterm`
    // (no args) launches the GUI; any argument dispatches to the CLI.
    if std::env::args().count() > 1 {
        cli::run(cli::parse_from_env());
        std::process::exit(0);
    }

    ensure_default_config();
    iced_app::run()
}

// ── Keyboard shortcut helpers (invoked from cli.rs Shortcuts group) ───────

pub(crate) fn shortcuts_list() {
    use iced::keyboard::Modifiers;

    let km = keymap::Keymap::from_defaults();
    for (mods, bk, action) in km.list_bindings() {
        let mods_str = format_mods(mods);
        let key_str = format_binding_key(&bk);
        let action_str = format_action(&action);
        println!("{:>20} {:>12} → {}", mods_str, key_str, action_str);
    }
}

pub(crate) fn shortcuts_preset(name: &str) {
    let content = match name {
        "default" => include_str!("../../../resources/presets/default.toml").to_string(),
        "vim" => include_str!("../../../resources/presets/vim.toml").to_string(),
        "tmux" => include_str!("../../../resources/presets/tmux.toml").to_string(),
        "iterm" => include_str!("../../../resources/presets/iterm.toml").to_string(),
        "vscode" => include_str!("../../../resources/presets/vscode.toml").to_string(),
        "agent-triage" => include_str!("../../../resources/presets/agent-triage.toml").to_string(),
        _ => {
            eprintln!("Unknown preset: {name}");
            eprintln!("Available: default, vim, tmux, iterm, vscode, agent-triage");
            std::process::exit(1);
        }
    };

    // Write preset as [[keyboard.bindings]] into the config file
    let config_path = app_config::AppConfig::config_path();
    write_keybindings_to_config(&config_path, &content);
    eprintln!("Applied preset '{name}' to {}", config_path.display());
    eprintln!("If PureTerm is running, keybindings will reload automatically.");
}

pub(crate) fn shortcuts_reset() {
    let config_path = app_config::AppConfig::config_path();
    write_keybindings_to_config(&config_path, "");
    eprintln!("Reset keybindings to defaults in {}", config_path.display());
}

/// Write the [[keyboard.bindings]] section to the config file.
/// Uses toml_edit to preserve the rest of the config.
fn write_keybindings_to_config(config_path: &std::path::Path, preset_content: &str) {
    use pureterm_settings::config_io;

    if let Err(e) = config_io::mutate_config(config_path, |doc| {
        // Remove existing keyboard section
        doc.remove("keyboard");

        // Parse the preset TOML and insert bindings
        if !preset_content.is_empty() {
            if let Ok(preset_doc) = preset_content.parse::<toml_edit::DocumentMut>() {
                if let Some(kb) = preset_doc.get("keyboard") {
                    doc.insert("keyboard", kb.clone());
                }
            }
        }
        Ok(())
    }) {
        eprintln!("Error writing config: {e}");
        std::process::exit(1);
    }
}

// ── Formatting helpers ──────────────────────────────────────────────────

fn format_mods(mods: iced::keyboard::Modifiers) -> String {
    let mut parts = Vec::new();
    if mods.contains(iced::keyboard::Modifiers::COMMAND) {
        parts.push("⌘");
    }
    if mods.contains(iced::keyboard::Modifiers::SHIFT) {
        parts.push("⇧");
    }
    if mods.contains(iced::keyboard::Modifiers::ALT) {
        parts.push("⌥");
    }
    if mods.contains(iced::keyboard::Modifiers::CTRL) {
        parts.push("⌃");
    }
    parts.join("")
}

fn format_binding_key(bk: &keymap::BindingKey) -> String {
    match bk {
        keymap::BindingKey::Character(s) => s.clone(),
        keymap::BindingKey::Named(nk) => match nk {
            keymap::BindingNamedKey::ArrowUp => "↑".into(),
            keymap::BindingNamedKey::ArrowDown => "↓".into(),
            keymap::BindingNamedKey::ArrowLeft => "←".into(),
            keymap::BindingNamedKey::ArrowRight => "→".into(),
            keymap::BindingNamedKey::Enter => "↵".into(),
            keymap::BindingNamedKey::Escape => "⎋".into(),
            keymap::BindingNamedKey::Tab => "⇥".into(),
            keymap::BindingNamedKey::Backspace => "⌫".into(),
            keymap::BindingNamedKey::Space => "Space".into(),
            keymap::BindingNamedKey::F(n) => format!("F{n}"),
            other => format!("{:?}", other),
        },
    }
}

fn format_action(action: &keymap::Action) -> String {
    match action {
        keymap::Action::NewTab => "New Tab".into(),
        keymap::Action::CloseTab => "Close Tab".into(),
        keymap::Action::ClosePaneOrTab => "Close Pane/Tab".into(),
        keymap::Action::NextTab => "Next Tab".into(),
        keymap::Action::PrevTab => "Prev Tab".into(),
        keymap::Action::ActivateTabByIndex(i) => format!("Tab {}", i + 1),
        keymap::Action::MovePaneToNewTab => "Pane → New Tab".into(),
        keymap::Action::MovePaneToNewWindow => "Pane → New Window".into(),
        keymap::Action::NewWindow => "New Window".into(),
        keymap::Action::NewWorkspace => "New Workspace".into(),
        keymap::Action::NextWorkspace => "Next Workspace".into(),
        keymap::Action::PrevWorkspace => "Previous Workspace".into(),
        keymap::Action::ActivateWorkspaceByIndex(i) => format!("Workspace {}", i + 1),
        keymap::Action::SplitHorizontal => "Split Horizontal".into(),
        keymap::Action::SplitVertical => "Split Vertical".into(),
        keymap::Action::SplitWeb => "Split Browser Pane".into(),
        keymap::Action::EqualizeSplits => "Equalize Splits".into(),
        keymap::Action::SwapPane => "Swap Pane".into(),
        keymap::Action::ToggleZoom => "Toggle Zoom".into(),
        keymap::Action::FocusPaneInOrder(d) => {
            if *d > 0 {
                "Focus Next Pane".into()
            } else {
                "Focus Prev Pane".into()
            }
        }
        keymap::Action::PaneFocusAdjacent(dir) => {
            format!("Focus Adjacent ({:?})", dir)
        }
        keymap::Action::ClipboardPaste => "Paste".into(),
        keymap::Action::ClipboardCopy => "Copy".into(),
        keymap::Action::SelectAll => "Select All".into(),
        keymap::Action::OpenSshDialog => "SSH Dialog".into(),
        keymap::Action::OpenCommandPalette => "Command Palette".into(),
        keymap::Action::OpenSettings => "Settings".into(),
        keymap::Action::FocusAddressBar => "Browser Address Bar".into(),
        keymap::Action::ToggleGitPanel => "Git Panel".into(),
        keymap::Action::ToggleLeftSidebar => "Left Sidebar".into(),
        keymap::Action::ToggleRightSidebar => "Right Sidebar".into(),
        keymap::Action::SidebarRightMode(m) => format!("Sidebar: {}", m.label()),
        keymap::Action::ToggleNotificationsPanel => "Notifications Panel".into(),
        keymap::Action::ErrorRecoveryToggle => "Error Recovery".into(),
        keymap::Action::ToggleAiChatPanel => "AI Chat Panel".into(),
        keymap::Action::ToggleInstantReplay => "Instant Replay".into(),
        keymap::Action::PrevMark => "Previous Mark".into(),
        keymap::Action::NextMark => "Next Mark".into(),
        keymap::Action::OpenHoverTarget => "Open URL/Path".into(),
        keymap::Action::OpenSemanticInEditor => "Open in Editor".into(),
        keymap::Action::PreviewFile => "Preview File".into(),
        keymap::Action::OpenLazygit => "Open Lazygit".into(),
        keymap::Action::IncreaseFontSize => "Font +".into(),
        keymap::Action::DecreaseFontSize => "Font −".into(),
        keymap::Action::ResetFontSize => "Font Reset".into(),
        keymap::Action::ToggleFullscreen => "Fullscreen".into(),
        keymap::Action::SpawnNewInstance => "New Instance".into(),
        keymap::Action::ToggleViMode => "Toggle Vi/Copy Mode".into(),
        keymap::Action::SearchForward => "Find".into(),
        keymap::Action::SearchInDirectory => "Find in Directory".into(),
        keymap::Action::SearchNext => "Find Next".into(),
        keymap::Action::SearchPrev => "Find Previous".into(),
        keymap::Action::SearchClose => "Close Find".into(),
        keymap::Action::SearchToggleCase => "Toggle Case-Sensitive".into(),
        keymap::Action::SearchToggleRegex => "Toggle Regex".into(),
        keymap::Action::SearchToggleWholeWord => "Toggle Whole Word".into(),
        keymap::Action::SearchToggleFollow => "Toggle Follow Tail".into(),
        keymap::Action::ReplayScrub(d) => format!("Replay Scrub ({d})"),
        keymap::Action::ToggleBroadcastInput => "Broadcast Input".into(),
        keymap::Action::ToggleQuakeDock => "Quake Dock".into(),
        keymap::Action::ToggleToolbelt => "Toolbelt".into(),
        // GAP-6.4: user-defined commands aren't part of the preset set,
        // but the match must be exhaustive for the `shortcuts` CLI.
        keymap::Action::CustomCommand(id) => format!("Custom: {id}"),
    }
}

// ── Config helpers ──────────────────────────────────────────────────────

fn ensure_default_config() {
    let config_dir = pureterm_config_dir();
    let config_file = config_dir.join("pureterm.toml");

    if config_file.exists() {
        return;
    }
    if config_dir.join("pureterm.toml").exists() {
        return;
    }
    if alacritty_config_exists() {
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&config_dir) {
        eprintln!("PureTerm: unable to create config dir {:?}: {e}", config_dir);
        return;
    }

    let default_config = include_str!("../default-config.toml");
    if let Err(e) = std::fs::write(&config_file, default_config) {
        eprintln!("PureTerm: unable to write default config {:?}: {e}", config_file);
    }
}

#[cfg(target_os = "macos")]
fn pureterm_config_dir() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_default().join(".config/pureterm")
}

#[cfg(not(target_os = "macos"))]
fn pureterm_config_dir() -> std::path::PathBuf {
    xdg::BaseDirectories::with_prefix("pureterm")
        .get_config_home()
        .unwrap_or_else(|_| std::path::PathBuf::from(".config/pureterm"))
}

#[cfg(target_os = "macos")]
fn alacritty_config_exists() -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".config/alacritty/alacritty.toml").exists()
        || home.join(".config/alacritty/alacritty.yml").exists()
        || home.join(".alacritty.toml").exists()
        || home.join(".alacritty.yml").exists()
}

#[cfg(not(target_os = "macos"))]
fn alacritty_config_exists() -> bool {
    xdg::BaseDirectories::with_prefix("alacritty")
        .find_config_file("alacritty.toml")
        .is_some()
}
