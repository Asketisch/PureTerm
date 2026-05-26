//! Alacritty-based runtime for PureTerm.
//!
//! This crate keeps Alacritty application modules close to upstream and exposes
//! `run_from_env` as the sole entrypoint.

#![warn(rust_2018_idioms, future_incompatible)]

#[cfg(not(any(feature = "x11", feature = "wayland", target_os = "macos", windows)))]
compile_error!(r#"at least one of the "x11"/"wayland" features must be enabled"#);

use std::error::Error;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::PathBuf;
use std::{env, fs};

use log::info;
#[cfg(windows)]
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, FreeConsole};
use winit::event_loop::EventLoop;
#[cfg(all(feature = "x11", not(any(target_os = "macos", windows))))]
use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

use alacritty_terminal::tty;

#[path = "alacritty/cli.rs"]
mod cli;
#[path = "alacritty/clipboard.rs"]
mod clipboard;
#[path = "alacritty/config/mod.rs"]
mod config;
#[path = "alacritty/daemon.rs"]
mod daemon;
#[path = "alacritty/display/mod.rs"]
mod display;
#[path = "alacritty/event.rs"]
mod event;
#[path = "alacritty/input/mod.rs"]
mod input;
#[path = "alacritty/logging.rs"]
mod logging;
#[cfg(target_os = "macos")]
#[path = "alacritty/macos/mod.rs"]
mod macos;
#[path = "alacritty/message_bar.rs"]
mod message_bar;
#[path = "alacritty/migrate/mod.rs"]
mod migrate;
#[cfg(windows)]
#[path = "alacritty/panic.rs"]
mod panic;
#[cfg(unix)]
#[path = "alacritty/polling/mod.rs"]
mod polling;
#[path = "alacritty/renderer/mod.rs"]
pub mod renderer;
#[path = "alacritty/scheduler.rs"]
mod scheduler;
#[path = "alacritty/string.rs"]
mod string;
#[path = "alacritty/shell_block.rs"]
mod shell_block;
#[path = "alacritty/shell_integration.rs"]
mod shell_integration;
#[path = "alacritty/trigger.rs"]
mod trigger;
#[path = "alacritty/trigger_dispatch.rs"]
mod trigger_dispatch;
#[path = "alacritty/window_context.rs"]
mod window_context;
#[path = "alacritty/ssh/mod.rs"]
mod ssh;

#[cfg(unix)]
use crate::cli::MessageOptions;
#[cfg(not(any(target_os = "macos", windows)))]
use crate::cli::SocketMessage;
use crate::cli::{Options, Subcommands};
use crate::config::UiConfig;
use crate::config::monitor::ConfigMonitor;
use crate::event::{Event, Processor};
#[cfg(target_os = "macos")]
use crate::macos::locale;
#[cfg(unix)]
use crate::polling::{IoListener, ipc};

/// Run the Alacritty runtime using process arguments.
pub fn run_from_env() -> Result<(), Box<dyn Error>> {
    #[cfg(windows)]
    panic::attach_handler();

    #[cfg(windows)]
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let options = Options::new();

    match options.subcommands {
        #[cfg(unix)]
        Some(Subcommands::Msg(options)) => msg(options)?,
        Some(Subcommands::Migrate(options)) => migrate::migrate(options),
        None => alacritty(options)?,
    }

    Ok(())
}

#[cfg(unix)]
#[allow(unused_mut)]
fn msg(mut options: MessageOptions) -> Result<(), Box<dyn Error>> {
    #[cfg(not(any(target_os = "macos", windows)))]
    if let SocketMessage::CreateWindow(window_options) = &mut options.message {
        window_options.activation_token =
            env::var("XDG_ACTIVATION_TOKEN").or_else(|_| env::var("DESKTOP_STARTUP_ID")).ok();
    }
    ipc::send_message(options.socket, options.message).map_err(|err| err.into())
}

struct TemporaryFiles {
    #[cfg(unix)]
    socket_path: Option<PathBuf>,
    log_file: Option<PathBuf>,
}

impl Drop for TemporaryFiles {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(socket_path) = self.socket_path.as_deref() {
            let _ = fs::remove_file(socket_path);
        }

        if let Some(log_file) = &self.log_file {
            if fs::remove_file(log_file).is_ok() {
                let _ = writeln!(io::stdout(), "Deleted log file at \"{}\"", log_file.display());
            }
        }
    }
}

fn alacritty(mut options: Options) -> Result<(), Box<dyn Error>> {
    let window_event_loop = EventLoop::<Event>::with_user_event().build()?;

    let log_file = match logging::initialize(&options, window_event_loop.create_proxy()) {
        Ok(log_file) => log_file,
        Err(_) => {
            log::set_max_level(options.log_level());
            None
        },
    };

    info!("Welcome to Alacritty");
    info!("Version {}", env!("VERSION"));

    #[cfg(all(feature = "x11", not(any(target_os = "macos", windows))))]
    info!(
        "Running on {}",
        if matches!(
            window_event_loop.display_handle().unwrap().as_raw(),
            RawDisplayHandle::Wayland(_)
        ) {
            "Wayland"
        } else {
            "X11"
        }
    );
    #[cfg(not(any(feature = "x11", target_os = "macos", windows)))]
    info!("Running on Wayland");

    let config = config::load(&mut options);
    log_config_path(&config);

    log::set_max_level(config.debug.log_level);

    tty::setup_env();

    for (key, value) in config.env.iter() {
        unsafe { env::set_var(key, value) };
    }

    #[cfg(target_os = "macos")]
    env::set_current_dir(home::home_dir().unwrap()).unwrap();

    #[cfg(target_os = "macos")]
    locale::set_locale_environment();

    #[cfg(target_os = "macos")]
    macos::disable_autofill();

    #[cfg(target_os = "macos")]
    macos::setup_menu();

    #[cfg(unix)]
    let socket_path = match IoListener::spawn(&config, &options, window_event_loop.create_proxy()) {
        Ok(handle) => handle.ipc_socket_path,
        Err(err) if options.daemon => return Err(err.into()),
        Err(err) => {
            log::warn!("Unable to create socket: {err:?}");
            None
        },
    };

    let log_cleanup = log_file.filter(|_| !config.debug.persistent_logging);
    let _files = TemporaryFiles {
        #[cfg(unix)]
        socket_path,
        log_file: log_cleanup,
    };

    let mut processor = Processor::new(config, options, &window_event_loop);
    let result = processor.run(window_event_loop);

    if let Some(config_monitor) = processor.config_monitor.take() {
        config_monitor.shutdown();
    }

    #[cfg(windows)]
    unsafe {
        FreeConsole();
    }

    info!("Goodbye");

    result
}

fn log_config_path(config: &UiConfig) {
    if config.config_paths.is_empty() {
        return;
    }

    let mut msg = String::from("Configuration files loaded from:");
    for path in &config.config_paths {
        let _ = write!(msg, "\n  {:?}", path.display());
    }
    info!("{msg}");
}
