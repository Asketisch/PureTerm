//! Multi-thread tokio entry point for `puretermd`.

use std::sync::Arc;

use crate::control::{self, ControlState};
use crate::discovery;
use crate::session::SessionManager;

/// Run the daemon until the process is killed / socket torn down.
///
/// Acquires the single-instance lock, builds a multi-thread tokio runtime, and
/// serves the JSON-RPC control plane. The per-session PTY pumps run on plain
/// std threads (see `session`); tokio only drives the control listener.
pub fn run() -> std::io::Result<()> {
    let _ = env_logger::try_init();
    discovery::ensure_dirs()?;

    // Single-instance: if another puretermd holds the lock, exit quietly (the
    // GUI's spawn-on-demand will then connect to the existing instance).
    let _lock = match discovery::acquire_instance_lock()? {
        Some(file) => file,
        None => {
            log::info!("another puretermd instance is running; exiting");
            return Ok(());
        }
    };

    let state = Arc::new(ControlState::new(SessionManager::default()));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("puretermd-worker")
        .build()?;

    runtime.block_on(control::serve(discovery::socket_path(), state))
}
