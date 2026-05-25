#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use std::path::PathBuf;

mod app;
mod config_io;
mod i18n;
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

    // Bring settings window to foreground on macOS (spawned as child process,
    // won't automatically activate without this)
    #[cfg(target_os = "macos")]
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        unsafe { macos_activate(); }
    });

    app::run(config_path)
}

#[cfg(target_os = "macos")]
unsafe fn macos_activate() {
    use std::ffi::c_void;

    unsafe extern "C" {
        fn objc_getClass(name: *const i8) -> *const c_void;
        fn sel_registerName(name: *const i8) -> *const c_void;
        fn objc_msgSend(obj: *const c_void, cmd: *const c_void, ...) -> *const c_void;
    }

    unsafe {
        let cls = objc_getClass(b"NSApplication\0".as_ptr() as *const i8);
        let shared = sel_registerName(b"sharedApplication\0".as_ptr() as *const i8);
        let app = objc_msgSend(cls, shared);

        let activate = sel_registerName(b"activateIgnoringOtherApps:\0".as_ptr() as *const i8);
        objc_msgSend(app, activate, true as usize);
    }
}

fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/pureterm/pureterm.toml")
}
