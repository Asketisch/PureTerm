mod app_config;
mod config_watcher;
mod iced_app;
mod ime_widget;
mod keymap;
mod split_tree;
mod status_bar;
#[cfg(target_os = "macos")]
mod macos_menu;
#[cfg(target_os = "macos")]
mod macos_reopen;
#[cfg(unix)]
mod ssh;
mod terminal_context;
mod terminal_shader;
mod vi_mode_handler;

fn main() -> iced::Result {
    env_logger::init();

    // GAP-3.7: Simple CLI dispatch before launching GUI.
    // Supports: pureterm shortcuts {list|preset NAME|reset}
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        handle_cli(&args[1..]);
        std::process::exit(0);
    }

    ensure_default_config();
    iced_app::run()
}

// ── CLI entry point ─────────────────────────────────────────────────────

fn handle_cli(args: &[String]) {
    match args.first().map(|s| s.as_str()) {
        Some("shortcuts") => shortcuts_cli(&args[1..]),
        _ => {
            eprintln!("PureTerm - GPU-accelerated terminal emulator");
            eprintln!();
            eprintln!("Usage: pureterm [COMMAND]");
            eprintln!();
            eprintln!("Commands:");
            eprintln!("  shortcuts list              List current key bindings");
            eprintln!("  shortcuts preset <NAME>     Apply a preset (default|vim|tmux|iterm|vscode|agent-triage)");
            eprintln!("  shortcuts reset             Reset to factory defaults");
        }
    }
}

fn shortcuts_cli(args: &[String]) {
    match args.first().map(|s| s.as_str()) {
        Some("list") => shortcuts_list(),
        Some("preset") => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or_else(|| {
                eprintln!("Usage: pureterm shortcuts preset <NAME>");
                eprintln!("Available: default, vim, tmux, iterm, vscode, agent-triage");
                std::process::exit(1);
            });
            shortcuts_preset(name);
        }
        Some("reset") => shortcuts_reset(),
        _ => {
            eprintln!("Usage: pureterm shortcuts <list|preset NAME|reset>");
        }
    }
}

fn shortcuts_list() {
    use iced::keyboard::Modifiers;

    let km = keymap::Keymap::from_defaults();
    for (mods, bk, action) in km.list_bindings() {
        let mods_str = format_mods(mods);
        let key_str = format_binding_key(&bk);
        let action_str = format_action(&action);
        println!("{:>20} {:>12} → {}", mods_str, key_str, action_str);
    }
}

fn shortcuts_preset(name: &str) {
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

fn shortcuts_reset() {
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
        keymap::Action::SplitHorizontal => "Split Horizontal".into(),
        keymap::Action::SplitVertical => "Split Vertical".into(),
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
        keymap::Action::IncreaseFontSize => "Font +".into(),
        keymap::Action::DecreaseFontSize => "Font −".into(),
        keymap::Action::ResetFontSize => "Font Reset".into(),
        keymap::Action::ToggleFullscreen => "Fullscreen".into(),
        keymap::Action::SpawnNewInstance => "New Instance".into(),
        keymap::Action::ToggleViMode => "Toggle Vi/Copy Mode".into(),
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
