#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use std::path::PathBuf;

mod app;
mod config_io;
mod pages;
mod theme;
mod widgets;

#[derive(Parser)]
#[command(name = "pureterm-settings", about = "PureTerm Settings UI")]
struct Cli {
    #[arg(long, short)]
    config: Option<PathBuf>,
}

fn main() -> iced::Result {
    env_logger::init();

    let cli = Cli::parse();
    let config_path = cli
        .config
        .or_else(config_io::find_config_path)
        .unwrap_or_else(default_config_path);

    log::info!("Using config: {}", config_path.display());

    app::run(config_path)
}

fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/pureterm/alacritty.toml")
}
