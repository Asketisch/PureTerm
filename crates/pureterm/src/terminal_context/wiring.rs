//! 守护进程(PTY)接线助手。
//!
//! `wire_local_daemon` / `wire_ssh_daemon` 是构造路径(create_local /
//! create_ssh)与恢复路径(attach_existing)共用的共享构造代码,用于把
//! 一个 `puretermd` 拥有的 PTY 字节流镜像接到本进程的 alacritty
//! `Term` + `PtyEventLoop` 上。`child_pid` 为 0(子进程归 daemon)。

use std::io;
use std::path::Path;
use std::sync::Arc;

use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Notifier};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Term};
use crossbeam_channel::bounded;

#[cfg(unix)]
use std::cell::RefCell;
#[cfg(unix)]
use std::sync::Mutex;

#[cfg(unix)]
use super::internal::{SshCtx, SshRuntime, ThreadJoiner};
use super::internal::LocalCtx;
use super::ChannelEventListener;
#[cfg(unix)]
use crate::ssh::config::SshProfile;
#[cfg(unix)]
use crate::ssh::reconnect::SshState;

/// GAP-5.1/5.6 shared wiring: build a `LocalCtx` that mirrors a daemon PTY
/// over `data_path`. Used by both the create path (fresh session, after
/// `create_local`) and the attach path (recovery of an existing session, after
/// `attach_existing`). `child_pid` is 0 — the child is owned by the daemon.
#[cfg(unix)]
pub(super) fn wire_local_daemon(
    data_path: &Path,
    size: TermSize,
    session_id: String,
) -> io::Result<LocalCtx> {
    use crate::daemon_pty::DaemonPty;

    let daemon_pty = DaemonPty::connect(data_path)?;

    let (event_tx, event_rx) = bounded(64);
    let event_listener = ChannelEventListener::new(event_tx);

    let term_config = TermConfig::default();
    let terminal = Term::new(term_config, &size, event_listener.clone());
    let terminal = Arc::new(FairMutex::new(terminal));

    let pty_loop = PtyEventLoop::new(
        Arc::clone(&terminal),
        event_listener,
        daemon_pty,
        false,
        false,
    )?;

    let loop_tx = pty_loop.channel();
    let _io_thread = pty_loop.spawn();

    Ok(LocalCtx {
        terminal,
        notifier: Notifier(loop_tx),
        event_rx,
        size,
        child_pid: 0, // child is owned by the daemon; pid not meaningful here
        session_id: Some(session_id),
    })
}

/// GAP-5.1/5.6 shared wiring: build an `SshCtx` that mirrors a daemon-owned
/// `ssh -tt` child over `data_path`. Used by both the create path (after
/// `create_ssh`) and the attach path (recovery). `password` is `None` on the
/// attach path: the live ssh child is already authenticated, and the password
/// is not persisted across GUI restart (auto-reconnect after a later network
/// drop on a restored session is therefore best-effort).
#[cfg(unix)]
pub(super) fn wire_ssh_daemon(
    data_path: &Path,
    size: TermSize,
    profile: &SshProfile,
    password: Option<String>,
    session_id: String,
) -> io::Result<SshCtx> {
    use crate::daemon_pty::DaemonPty;

    let daemon_pty = DaemonPty::connect(data_path)?;

    let (event_tx, event_rx) = bounded(64);
    let event_listener = ChannelEventListener::new(event_tx);

    let term_config = TermConfig::default();
    let terminal = Term::new(term_config, &size, event_listener.clone());
    let terminal = Arc::new(FairMutex::new(terminal));

    let pty_loop = PtyEventLoop::new(
        Arc::clone(&terminal),
        event_listener,
        daemon_pty,
        false,
        false,
    )?;
    let loop_tx = pty_loop.channel();
    let io_thread = ThreadJoiner::new(pty_loop.spawn());

    Ok(SshCtx {
        terminal,
        runtime: RefCell::new(SshRuntime {
            notifier: Notifier(loop_tx),
            event_rx,
            io_thread: Some(io_thread),
            stderr_file: None,
        }),
        size,
        profile_display: profile.name.clone(),
        ssh_host: profile.host.clone(),
        ssh_user: profile.user.clone(),
        ssh_port: profile.port,
        ssh_key_path: profile.key_path.clone(),
        ssh_password: password,
        reconnect_state: Arc::new(Mutex::new(SshState::Connecting)),
        user_initiated_close: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        session_id: Some(session_id),
        forward_summary: profile.forwards().summary(),
        ssh_forwards: profile.forwards(),
        ssh_jump_hosts: profile.jump_hosts.clone(),
    })
}
