mod iced_app;
mod terminal_context;
mod terminal_shader;

fn main() -> iced::Result {
    env_logger::init();
    ensure_default_config();
    iced_app::run()
}

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
