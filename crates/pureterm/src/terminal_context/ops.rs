//! `impl TerminalContext` 的运行时操作集合。
//!
//! 按变体派发的查询/IO/状态方法:`terminal` / `write_to_pty` /
//! `pty_notifier` / `resize` / `poll_terminal_event` / `is_ssh` /
//! `child_pid` / `session_id` / `profile_display` / `ssh_release_key` /
//! `read_stderr`,以及 GAP-5.6 重连支持(`ssh_state_snapshot` /
//! `set_ssh_state` / `mark_user_initiated_close` / `is_user_initiated_close`
//! / `ssh_reconnect_params` / `reattach`)。

use std::io;
use std::io::Read as _;
use std::sync::Arc;

#[cfg(unix)]
use std::sync::atomic::Ordering;

use alacritty_terminal::event::{Notify, WindowSize};
use alacritty_terminal::event_loop::{Msg, Notifier};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::Term;

#[cfg(unix)]
use std::fs::File;

use super::marker::inject_reconnect_marker_into;
use super::ChannelEventListener;
use super::TerminalContext;
#[cfg(unix)]
use crate::ssh::pty::SshPty;
#[cfg(unix)]
use crate::ssh::reconnect::{ReconnectParams, SshState, SshStateSnapshot};

impl TerminalContext {
    /// Access the underlying `Term` grid. The shader uses this to read
    /// cells every frame. Both variants expose the same grid shape.
    pub fn terminal(&self) -> &Arc<FairMutex<Term<ChannelEventListener>>> {
        match self {
            TerminalContext::Local(ctx) => &ctx.terminal,
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => &ctx.terminal,
        }
    }

    /// Write bytes to the PTY (keyboard input).
    pub fn write_to_pty(&self, bytes: &[u8]) {
        match self {
            TerminalContext::Local(ctx) => ctx.notifier.notify(bytes.to_vec()),
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => ctx.runtime.borrow().notifier.notify(bytes.to_vec()),
        }
    }

    /// GAP-42: return a clone of the PTY notifier so login commands can be
    /// sent from a background thread without holding a non-`Send`
    /// `Arc<TerminalContext>`.
    pub fn pty_notifier(&self) -> Option<Notifier> {
        match self {
            TerminalContext::Local(ctx) => Some(ctx.notifier.clone()),
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => Some(ctx.runtime.borrow().notifier.clone()),
        }
    }

    /// GAP-5.6: stamp a "reconnected" separator line into this SSH pane's grid
    /// on a successful reconnect. No-op for local panes. See
    /// [`inject_reconnect_marker_into`].
    pub fn inject_reconnect_marker(&self) {
        match self {
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => {
                inject_reconnect_marker_into(&ctx.terminal, &ctx.ssh_host);
            },
            // Local pane (or non-unix): nothing to mark.
            _ => {},
        }
    }

    /// Resize the terminal grid and notify the PTY.
    pub fn resize(&self, cols: usize, rows: usize, cell_w: u16, cell_h: u16) {
        let size = TermSize::new(cols, rows);
        match self {
            TerminalContext::Local(ctx) => {
                // 使用 unfair lock 避免 UI 线程在 PTY 读取线程持有锁时阻塞
                ctx.terminal.lock_unfair().resize(size);
                let window_size = WindowSize {
                    num_cols: cols as u16,
                    num_lines: rows as u16,
                    cell_width: cell_w,
                    cell_height: cell_h,
                };
                let _ = ctx.notifier.0.send(Msg::Resize(window_size));
            },
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => {
                ctx.terminal.lock_unfair().resize(size);
                let window_size = WindowSize {
                    num_cols: cols as u16,
                    num_lines: rows as u16,
                    cell_width: cell_w,
                    cell_height: cell_h,
                };
                let _ = ctx.runtime.borrow().notifier.0.send(Msg::Resize(window_size));
            },
        }
    }

    /// Poll for one pending terminal event (drained by the iced loop each
    /// wakeup). Replaces the old `event_rx()` accessor: because the SSH
    /// receiver now lives behind the swappable `runtime` `RefCell`, there is
    /// no stable `&Receiver` to hand out.
    pub fn poll_terminal_event(&self) -> Option<super::TerminalEvent> {
        match self {
            TerminalContext::Local(ctx) => ctx.event_rx.try_recv().ok(),
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => ctx.runtime.borrow().event_rx.try_recv().ok(),
        }
    }

    /// True if this context is backed by an SSH connection.
    pub fn is_ssh(&self) -> bool {
        match self {
            TerminalContext::Local(_) => false,
            #[cfg(unix)]
            TerminalContext::Ssh(_) => true,
        }
    }

    /// Return the child PID (only meaningful for local PTY contexts).
    pub fn child_pid(&self) -> Option<u32> {
        match self {
            TerminalContext::Local(ctx) => Some(ctx.child_pid),
            #[cfg(unix)]
            TerminalContext::Ssh(_) => None,
        }
    }

    /// GAP-5.6: the daemon session id if this context mirrors a `puretermd`-
    /// owned PTY (local or ssh). `None` for a direct PTY, which cannot survive
    /// GUI restart and is therefore not persisted/restored.
    pub fn session_id(&self) -> Option<&str> {
        match self {
            TerminalContext::Local(ctx) => ctx.session_id.as_deref(),
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => ctx.session_id.as_deref(),
        }
    }

    /// GAP-40: active SSH forward summary (`L2 R1 X`), if any.
    #[cfg(unix)]
    pub fn ssh_forward_summary(&self) -> Option<&str> {
        match self {
            TerminalContext::Ssh(ctx) => ctx.forward_summary.as_deref(),
            _ => None,
        }
    }

    /// Human-readable profile name (only meaningful for SSH contexts).
    pub fn profile_display(&self) -> Option<&str> {
        match self {
            TerminalContext::Local(_) => None,
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => Some(&ctx.profile_display),
        }
    }

    /// SSH connection key fields for pool release on close.
    /// Returns `None` for local contexts.
    #[cfg(unix)]
    pub fn ssh_release_key(&self) -> Option<(&str, Option<&str>, Option<u16>)> {
        match self {
            TerminalContext::Local(_) => None,
            TerminalContext::Ssh(ctx) => Some((
                &ctx.ssh_host,
                ctx.ssh_user.as_deref(),
                ctx.ssh_port,
            )),
        }
    }

    /// Read available stderr bytes from the SSH child process (non-blocking).
    /// Returns `None` if this is not an SSH context or the stderr pipe has
    /// been consumed / was never captured.
    #[cfg(unix)]
    pub fn read_stderr(&self) -> Option<String> {
        match self {
            TerminalContext::Local(_) => None,
            TerminalContext::Ssh(ctx) => {
                let mut rt = ctx.runtime.borrow_mut();
                let file = rt.stderr_file.as_mut()?;
                let mut buf = vec![0u8; 4096];
                let mut accumulated = Vec::new();
                // The pipe is non-blocking; read until WouldBlock.
                loop {
                    match file.read(&mut buf) {
                        Ok(0) => break, // EOF
                        Ok(n) => accumulated.extend_from_slice(&buf[..n]),
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }
                if accumulated.is_empty() {
                    return None;
                }
                Some(String::from_utf8_lossy(&accumulated).into_owned())
            },
        }
    }

    // ── GAP-5.6 reconnect support ──────────────────────────────────────────

    /// Snapshot the SSH reconnect state for UI rendering. `None` for local.
    #[cfg(unix)]
    pub fn ssh_state_snapshot(&self) -> Option<SshStateSnapshot> {
        match self {
            TerminalContext::Local(_) => None,
            TerminalContext::Ssh(ctx) => {
                Some(SshStateSnapshot::from(&*ctx.reconnect_state.lock().unwrap()))
            },
        }
    }

    /// Set the SSH reconnect state (called from the iced loop on
    /// [`crate::ssh::reconnect::ReconnectUpdate`] Progress/Failed, or on
    /// `reattach`). No-op for local contexts.
    #[cfg(unix)]
    pub fn set_ssh_state(&self, state: SshState) {
        if let TerminalContext::Ssh(ctx) = self {
            *ctx.reconnect_state.lock().unwrap() = state;
        }
    }

    /// Mark this SSH pane as closing by user intent (manual disconnect / tab
    /// close). MUST be set before writing `exit\r` so the exit handler does
    /// not auto-reconnect. No-op for local contexts.
    #[cfg(unix)]
    pub fn mark_user_initiated_close(&self) {
        if let TerminalContext::Ssh(ctx) = self {
            ctx.user_initiated_close.store(true, Ordering::SeqCst);
        }
    }

    /// True iff a user-initiated close was flagged for this SSH pane.
    #[cfg(unix)]
    pub fn is_user_initiated_close(&self) -> bool {
        match self {
            TerminalContext::Local(_) => false,
            TerminalContext::Ssh(ctx) => ctx.user_initiated_close.load(Ordering::SeqCst),
        }
    }

    /// Clone out the parameters needed to re-spawn this SSH connection.
    /// `None` for local contexts.
    #[cfg(unix)]
    pub fn ssh_reconnect_params(&self) -> Option<ReconnectParams> {
        match self {
            TerminalContext::Local(_) => None,
            TerminalContext::Ssh(ctx) => Some(ReconnectParams {
                host: ctx.ssh_host.clone(),
                user: ctx.ssh_user.clone(),
                port: ctx.ssh_port,
                key_path: ctx.ssh_key_path.clone(),
                password: ctx.ssh_password.clone(),
                forward_agent: false,
                forwards: ctx.ssh_forwards.clone(),
                jump_hosts: ctx.ssh_jump_hosts.clone(),
            }),
        }
    }

    /// GAP-5.6: reattach a freshly-spawned `SshPty` to this context after a
    /// reconnect. Joins the old (dead) loop thread, builds a new
    /// `PtyEventLoop` against the SAME terminal `Arc` (so scrollback is
    /// preserved), and swaps the notifier / event receiver / io thread /
    /// stderr.
    ///
    /// Soundness relies on alacritty's `Term::exit` (term/mod.rs) NOT setting
    /// a sticky "exited" flag — it only emits `Event::Exit`. The `PtyEventLoop`
    /// owns its own `Poller` (loop-local), so the old dead loop leaves no
    /// registrations on the terminal. If a future alacritty fork adds a
    /// sticky exited flag, reattach will silently break (no input processed)
    /// — re-audit then.
    #[cfg(unix)]
    pub fn reattach(&self, ssh_pty: SshPty, stderr: Option<File>) -> io::Result<()> {
        use alacritty_terminal::tty::{EventedPty, EventedReadWrite};
        use crossbeam_channel::bounded;

        let TerminalContext::Ssh(ctx) = self else {
            return Ok(());
        };

        let mut rt = ctx.runtime.borrow_mut();

        // 1. Join the old loop thread. It has already exited (the child exit
        //    is what triggered the reconnect), but joining closes the small
        //    window between "Exit event delivered" and "thread fully torn
        //    down".
        if let Some(mut joiner) = rt.io_thread.take() {
            joiner.join();
        }

        // 2. Fresh channel pair + listener for the new loop.
        let (event_tx, event_rx) = bounded(64);
        let event_listener = ChannelEventListener::new(event_tx);

        // 3. New loop against the same terminal Arc (scrollback preserved).
        let pty_loop = alacritty_terminal::event_loop::EventLoop::new(
            Arc::clone(&ctx.terminal),
            event_listener,
            ssh_pty,
            false,
            false,
        )?;
        let loop_tx = pty_loop.channel();
        let io_thread = super::internal::ThreadJoiner::new(pty_loop.spawn());

        // 4. Swap the swappable runtime state.
        rt.notifier = Notifier(loop_tx);
        rt.event_rx = event_rx;
        rt.io_thread = Some(io_thread);
        rt.stderr_file = stderr;
        Ok(())
    }
}
