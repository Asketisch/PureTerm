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

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("puretermd-worker")
        .build()?;

    // Build the runtime before the control state so GAP-5.2 tunnel tasks can be
    // spawned onto it (ControlState holds the handle).
    let state = Arc::new(ControlState::with_runtime(
        SessionManager::default(),
        runtime.handle().clone(),
    ));

    // GAP-5.2: open the TOFU known_hosts store and hand it to the tunnel
    // manager. A failure to open (rare: data dir unreadable) degrades to the
    // legacy accept-all host-key behavior rather than crashing the daemon.
    match crate::known_hosts::KnownHosts::open(crate::discovery::known_hosts_file()) {
        Ok(kh) => state.tunnels.set_known_hosts(Arc::new(kh)),
        Err(e) => log::warn!("known_hosts: open failed, TOFU disabled: {e}"),
    }

    // GAP-5.2: config-only restore — reload tunnel configs so they survive a
    // daemon restart, registered as Stopped (no listener/task). The user
    // re-starts them explicitly via tunnel.start. Secrets were never persisted,
    // so a password-auth tunnel re-enters NeedAuth on re-start.
    if let Some(file) = crate::persist::load_tunnels(&crate::discovery::tunnels_file()) {
        let n = file.tunnels.len();
        for cfg in file.tunnels {
            state.tunnels.register_stopped(cfg);
        }
        if n > 0 {
            log::info!("restored {n} tunnel config(s) (config-only; use tunnel.start to connect)");
        }
    }

    // GAP-7.3: periodic CPU sample tick drives `cpu_idle_secs` for every
    // session — the dual idle condition for hibernation, alongside PTY-output
    // idle recorded by the pump. Runs concurrently with the control listener.
    let tick_state = state.clone();
    runtime.spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            tick_state.manager.sample_cpu_pass();
        }
    });

    runtime.block_on(control::serve(discovery::socket_path(), state))
}
