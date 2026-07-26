//! `impl TerminalContext` 的构造器集合(`new_local*` / `new_ssh*` /
//! `attach_*` / `new_local_auto`)。
//!
//! 这些方法构造上下文对象:本机直接 PTY、本机 daemon-owned PTY、
//! SSH 直连 PTY、SSH daemon-owned PTY,以及跨重启恢复用的 attach 路径。
//! 方法的对外语义保持与 P2 之前一致,只是物理上独立成文件。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
use std::sync::Mutex;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Notifier};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty::{self, Options as PtyOptions};
use crossbeam_channel::bounded;
// GAP-15: shell-integration injection opts (OSC 7 + OSC 133).
use pureterm_daemon::shell_integration::ShellIntegrationOpts;

#[cfg(unix)]
use std::cell::RefCell;
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::sync::atomic::AtomicBool;

#[cfg(unix)]
use super::internal::{SshCtx, SshRuntime, ThreadJoiner};
use super::internal::LocalCtx;
#[cfg(unix)]
use super::wiring::{wire_local_daemon, wire_ssh_daemon};
use super::ChannelEventListener;
use super::TerminalContext;
#[cfg(unix)]
use crate::ssh::config::SshProfile;
#[cfg(unix)]
use crate::ssh::pty::SshPty;
#[cfg(unix)]
use crate::ssh::reconnect::SshState;

impl TerminalContext {
    /// Create a new local-pty terminal context. Equivalent to the
    /// pre-P2 `TerminalContext::new()`.
    ///
    /// `cwd`, when `Some`, becomes the shell's startup directory so a new
    /// tab/pane can inherit the current working directory. `shell_opts` (GAP-15)
    /// selects which shells get OSC 133 integration injected alongside OSC 7.
    pub fn new_local(cwd: Option<&Path>, shell_opts: ShellIntegrationOpts) -> io::Result<Self> {
        let (event_tx, event_rx) = bounded(64);
        let event_listener = ChannelEventListener::new(event_tx);

        let num_cols: u16 = 80;
        let num_lines: u16 = 24;
        let cell_width: u16 = 8;
        let cell_height: u16 = 16;
        let size = TermSize::new(num_cols as usize, num_lines as usize);

        let term_config = TermConfig::default();
        let terminal = Term::new(term_config, &size, event_listener.clone());
        let terminal = Arc::new(FairMutex::new(terminal));

        let window_size = WindowSize {
            num_lines,
            num_cols,
            cell_width,
            cell_height,
        };
        tty::setup_env();

        // Build PTY options: set the startup directory to the inherited cwd,
        // and inject the OSC 7 cwd-reporting hook into the child environment
        // (shared with the daemon path) so this shell reports its working
        // directory — which is what lets new tabs/panes inherit it.
        //
        // Using `Options::env` instead of `std::env::set_var` on this process
        // avoids two problems the previous approach had: (1) racing concurrent
        // PTY spawns on the same temp ZDOTDIR, and (2) leaking the temp
        // ZDOTDIR into unrelated spawns. It also keeps the hook injection
        // out of `Msg::Input`, which can block the PTY IO thread on macOS
        // .app bundles when the payload exceeds ~80 bytes.
        let mut pty_options = PtyOptions::default();
        pty_options.working_directory = cwd.map(PathBuf::from);
        // GAP-15: OSC 7 cwd + OSC 133 marks. `shell_opts` comes from the loaded
        // `[shell_integration]` config (the daemon path can't honor config, so
        // it always uses all_on — this direct fallback is where the knobs apply).
        pty_options.env.extend(
            pureterm_daemon::shell_integration::prepare_shell_env(
                &std::process::id().to_string(),
                shell_opts,
            ),
        );

        let pty = tty::new(&pty_options, window_size, 0)?;

        let child_pid = pty.child().id();

        let pty_loop = PtyEventLoop::new(
            Arc::clone(&terminal),
            event_listener,
            pty,
            false,
            false,
        )?;

        let loop_tx = pty_loop.channel();
        let _io_thread = pty_loop.spawn();

        Ok(TerminalContext::Local(LocalCtx {
            terminal,
            notifier: Notifier(loop_tx),
            event_rx,
            size,
            child_pid,
            session_id: None,
        }))
    }

    /// Create a new local-PTY terminal context routed through the persistent
    /// daemon (`puretermd`). The daemon owns the real PTY (so the child
    /// survives GUI close); this side holds a `DaemonPty` that mirrors bytes
    /// over a socketpair.
    ///
    /// GAP-5.1 M1: same contract as `new_local()` from the caller's
    /// perspective — returns a `TerminalContext::Local` with a working
    /// terminal, notifier, and event channel.
    #[cfg(unix)]
    pub fn new_local_via_daemon(cwd: Option<&Path>) -> io::Result<Self> {
        use crate::daemon_client;

        let daemon_socket = daemon_client::ensure_daemon()?;

        let num_cols: u16 = 80;
        let num_lines: u16 = 24;
        let size = TermSize::new(num_cols as usize, num_lines as usize);

        let (session_id, data_path) = daemon_client::create_local(
            &daemon_socket,
            num_cols,
            num_lines,
            cwd.and_then(|p| p.to_str()),
            None,
            None,
            crate::ssh::config::load_ssh_settings(&crate::app_config::AppConfig::config_path())
                .smallest_screen_wins,
        )?;

        let ctx = wire_local_daemon(&data_path, size, session_id)?;
        Ok(TerminalContext::Local(ctx))
    }

    /// GAP-5.6 cross-restart recovery: re-attach to a daemon session that was
    /// created in a previous GUI run and is still alive. The daemon replays the
    /// session's scrollback as `SCROLLBACK` frames, so the freshly-built `Term`
    /// + `vte::ansi::Processor` rebuilds the visible grid + scrollback. The
    /// shell continues running uninterrupted (no re-spawn).
    #[cfg(unix)]
    pub fn attach_local_via_daemon(session_id: &str, size: TermSize) -> io::Result<Self> {
        use crate::daemon_client;

        let daemon_socket = daemon_client::ensure_daemon()?;
        let data_path = daemon_client::attach_existing(&daemon_socket, session_id)?;
        let ctx = wire_local_daemon(&data_path, size, session_id.to_string())?;
        Ok(TerminalContext::Local(ctx))
    }

    /// GAP-5.1 M1: route new local terminals through the persistent daemon so
    /// the PTY survives GUI restart. Falls back to a direct local PTY when the
    /// daemon is unreachable (e.g. `puretermd` is not packaged alongside the
    /// running binary), so the terminal always works — at the cost of losing
    /// session-survival until the daemon is available. This is the entry point
    /// the GUI tab/split creation should use.
    ///
    /// `cwd`, when `Some`, is forwarded to whichever path spawns the shell so
    /// a new tab/pane can inherit the current working directory. `shell_opts`
    /// (GAP-15) is honored only on the direct-PTY fallback (the daemon path is
    /// always full integration).
    pub fn new_local_auto(cwd: Option<&Path>, shell_opts: ShellIntegrationOpts) -> Self {
        #[cfg(unix)]
        {
            match Self::new_local_via_daemon(cwd) {
                Ok(ctx) => return ctx,
                Err(e) => log::warn!(
                    "puretermd unavailable ({e}); using direct local PTY \
                     (session will not survive GUI restart)"
                ),
            }
        }
        #[cfg(not(unix))]
        {
            log::debug!("daemon PTY path is unix-only; using direct local PTY");
        }
        Self::new_local(cwd, shell_opts).expect("Failed to create terminal context")
    }

    /// Create a new SSH-backed terminal context. The SSH child is spawned
    /// via `SshSessionManager::spawn_pty`, then wired into the same
    /// alacritty `PtyEventLoop` the local path uses.
    ///
    /// This is the entry point the GAP-5.7 P3 stage will call. It is
    /// unix-only; on Windows the caller must use a stub instead.
    #[cfg(unix)]
    pub fn new_ssh(
        profile: &SshProfile,
        ssh_pty: SshPty,
        socket_path: PathBuf,
        password: Option<String>,
        stderr: Option<File>,
    ) -> io::Result<Self> {
        use alacritty_terminal::tty::{EventedPty, EventedReadWrite};

        let (event_tx, event_rx) = bounded(64);
        let event_listener = ChannelEventListener::new(event_tx);

        let num_cols: u16 = 80;
        let num_lines: u16 = 24;
        let cell_width: u16 = 8;
        let cell_height: u16 = 16;
        let size = TermSize::new(num_cols as usize, num_lines as usize);

        let term_config = TermConfig::default();
        let terminal = Term::new(term_config, &size, event_listener.clone());
        let terminal = Arc::new(FairMutex::new(terminal));

        // Hand the concrete SshPty to PtyEventLoop. The loop thread owns the
        // pty by value for its full lifetime; the `ThreadJoiner` lets a later
        // `reattach` wait for this thread before spinning a replacement loop.
        let pty_loop = PtyEventLoop::new(
            Arc::clone(&terminal),
            event_listener,
            ssh_pty,
            false,
            false,
        )?;

        let loop_tx = pty_loop.channel();
        let io_thread = ThreadJoiner::new(pty_loop.spawn());

        // socket_path is recorded in the SshSessionManager pool already.
        let _ = socket_path;

        Ok(TerminalContext::Ssh(SshCtx {
            terminal,
            runtime: RefCell::new(SshRuntime {
                notifier: Notifier(loop_tx),
                event_rx,
                io_thread: Some(io_thread),
                stderr_file: stderr,
            }),
            size,
            profile_display: profile.name.clone(),
            ssh_host: profile.host.clone(),
            ssh_user: profile.user.clone(),
            ssh_port: profile.port,
            ssh_key_path: profile.key_path.clone(),
            ssh_password: password,
            reconnect_state: Arc::new(Mutex::new(SshState::Connecting)),
            user_initiated_close: Arc::new(AtomicBool::new(false)),
            session_id: None,
            forward_summary: profile.forwards().summary(),
            ssh_forwards: profile.forwards(),
        ssh_jump_hosts: profile.jump_hosts.clone(),
        }))
    }

    /// GAP-5.1 M3: open an SSH session owned by the persistent daemon so the
    /// remote shell survives GUI restart. The GUI side is the same `DaemonPty`
    /// mirror the local path uses — only the daemon spawns `ssh` instead of a
    /// local shell. `password` must already be resolved (CredentialStore /
    /// profile) by the caller; it is forwarded to the daemon for `SSH_ASKPASS`.
    ///
    /// Auto-reconnect (GAP-5.6) is intentionally NOT wired here: the daemon
    /// keeps the existing ssh child alive across GUI restart, and re-attach
    /// rebuilds scrollback. If the network itself drops, the user reconnects via
    /// the SSH dialog (daemon-side reconnect is a later refinement).
    #[cfg(unix)]
    pub fn new_ssh_via_daemon(profile: &SshProfile, password: Option<String>) -> io::Result<Self> {
        use crate::daemon_client;

        let daemon_socket = daemon_client::ensure_daemon()?;
        let num_cols: u16 = 80;
        let num_lines: u16 = 24;
        let size = TermSize::new(num_cols as usize, num_lines as usize);

        let settings =
            crate::ssh::config::load_ssh_settings(&crate::app_config::AppConfig::config_path());

        let (session_id, data_path) = daemon_client::create_ssh(
            &daemon_socket,
            &profile.host,
            profile.user.as_deref(),
            profile.port,
            profile.key_path.as_deref(),
            password.as_deref(),
            num_cols,
            num_lines,
            profile.forward_agent,
            // ssh.agent_sock config override wiring is a follow-up; the
            // resolver already honors it (override > $SSH_AUTH_SOCK >
            // launchctl) — pass None to use inherited env / launchctl.
            None,
            profile.forwards(),
            settings.smallest_screen_wins,
        )?;

        let ctx = wire_ssh_daemon(&data_path, size, profile, password, session_id)?;
        Ok(TerminalContext::Ssh(ctx))
    }

    /// GAP-5.6 cross-restart recovery: re-attach to a daemon-owned SSH session
    /// that is still alive (the `ssh -tt` child survived GUI restart in the
    /// daemon). No re-authentication is needed — the remote shell continues.
    /// `profile` supplies the display name / connection metadata (looked up by
    /// name from the GUI's persisted profile list at restore time; if the
    /// profile was deleted, the caller passes a synthetic placeholder and
    /// auto-reconnect after a later drop is best-effort).
    #[cfg(unix)]
    pub fn attach_ssh_via_daemon(
        session_id: &str,
        profile: &SshProfile,
        size: TermSize,
    ) -> io::Result<Self> {
        use crate::daemon_client;

        let daemon_socket = daemon_client::ensure_daemon()?;
        let data_path = daemon_client::attach_existing(&daemon_socket, session_id)?;
        let ctx = wire_ssh_daemon(&data_path, size, profile, None, session_id.to_string())?;
        Ok(TerminalContext::Ssh(ctx))
    }
}
