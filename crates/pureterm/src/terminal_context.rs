use std::cell::RefCell;
use std::fs::File;
use std::io;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
use std::sync::atomic::AtomicBool;
#[cfg(unix)]
use std::sync::Mutex;

#[cfg(unix)]
use alacritty_terminal::tty::EventedPty as _;
use alacritty_terminal::event::{EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty::{self, Options as PtyOptions};
use alacritty_terminal::vte::ansi;
use crossbeam_channel::{bounded, Receiver, Sender};
// GAP-15: shell-integration injection opts (OSC 7 + OSC 133).
use pureterm_daemon::shell_integration::ShellIntegrationOpts;
#[cfg(unix)]
use pureterm_ssh::SshForwards;

#[cfg(unix)]
use crate::ssh::config::SshProfile;
#[cfg(unix)]
use crate::ssh::pty::SshPty;
#[cfg(unix)]
use crate::ssh::reconnect::{ReconnectParams, SshState, SshStateSnapshot};

/// Terminal event types sent from PTY thread to Iced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    Wakeup,
    Exit,
    CwdChanged(String),
    /// GAP-1.3: window title set via OSC 0/2, or restored by CSI 23t (pop).
    Title(String),
    /// GAP-1.3: title cleared (OSC reset) — fall back to the derived tab title.
    ResetTitle,
    /// GAP-15: OSC 133 shell-integration mark (prompt/command/output boundary,
    /// with exit code + duration on OutputEnd). Surfaced so the GUI can track
    /// per-pane commands and emit `shell.*` events for the AI context pipeline.
    ShellMark(alacritty_terminal::event::ShellMark),
    /// GAP-09: OSC 9/99/777 desktop notification parsed from the PTY.
    Notification(alacritty_terminal::event::Notification),
    /// GAP-09: terminal bell (0x07) — surfaced as an additional "needs
    /// attention" signal (was previously dropped by the `_ => return` below).
    Bell,
    /// T-core-graphics: a decoded graphics image (Sixel / Kitty / iTerm2
    /// inline). Surfaced so the GUI's `ImageStore` can pick it up and the
    /// renderer can blit it onto the grid.
    GraphicsImage(alacritty_terminal::event::GraphicsImage),
}

/// Bridge: implements `EventListener` by sending through a crossbeam channel.
#[derive(Clone)]
pub struct ChannelEventListener {
    tx: Sender<TerminalEvent>,
}

impl ChannelEventListener {
    pub fn new(tx: Sender<TerminalEvent>) -> Self {
        Self { tx }
    }
}

impl EventListener for ChannelEventListener {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        use alacritty_terminal::event::Event as TEvent;
        let mapped = match event {
            TEvent::Wakeup => TerminalEvent::Wakeup,
            TEvent::Exit => TerminalEvent::Exit,
            TEvent::ChildExit(_) => TerminalEvent::Exit,
            TEvent::CwdChanged(path) => TerminalEvent::CwdChanged(path),
            // GAP-1.3: surface the program-set window title (OSC 0/2 + CSI
            // 22t/23t push/pop, all routed through `Term::set_title` upstream)
            // so the GUI can show it instead of only the derived tab title.
            TEvent::Title(title) => TerminalEvent::Title(title),
            TEvent::ResetTitle => TerminalEvent::ResetTitle,
            // GAP-15: forward OSC 133 shell-integration marks to the GUI (was
            // previously dropped by the `_ => return` catch-all below).
            TEvent::ShellMark(m) => TerminalEvent::ShellMark(m),
            // GAP-09: forward OSC 9/99/777 notifications and the terminal bell.
            // Both MUST precede the `_ => return` catch-all below or they are
            // silently dropped — the same trap that bit ShellMark pre-GAP-15.
            TEvent::Notification(n) => TerminalEvent::Notification(n),
            TEvent::Bell => TerminalEvent::Bell,
            // T-core-graphics: forward decoded graphics images to the GUI.
            // The GUI's `ImageStore` decides whether to blit (it owns the
            // eviction + scrollback-prune policies).
            TEvent::GraphicsImage(img) => TerminalEvent::GraphicsImage(img),
            _ => return,
        };
        let _ = self.tx.send(mapped);
    }
}

/// Local PTY context: owns the user's shell PTY.
struct LocalCtx {
    terminal: Arc<FairMutex<Term<ChannelEventListener>>>,
    notifier: Notifier,
    event_rx: Receiver<TerminalEvent>,
    size: TermSize,
    child_pid: u32,
    /// GAP-5.6: the daemon session id when this context is a thin mirror over
    /// a `puretermd`-owned PTY (set by the daemon paths). `None` for a direct
    /// local PTY (the `new_local` fallback), which cannot survive GUI restart
    /// and is therefore not persisted/restored.
    session_id: Option<String>,
}

/// SSH PTY context: owns a remote ssh -tt child.
#[cfg(unix)]
struct SshCtx {
    terminal: Arc<FairMutex<Term<ChannelEventListener>>>,
    /// Swappable per-connection runtime (notifier channel + IO thread +
    /// stderr). Wrapped in `RefCell` because `TerminalContext` lives behind
    /// an `Arc` shared with the render path, so `reattach` mutates it through
    /// interior mutability.
    runtime: RefCell<SshRuntime>,
    size: TermSize,
    /// Human-readable profile (kept for the tab title).
    profile_display: String,
    /// Connection key fields for pool release on close + reconnect.
    ssh_host: String,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    /// Identity file (-i) used to authenticate; replayed on reconnect.
    ssh_key_path: Option<String>,
    /// Resolved password (CredentialStore > TOML plaintext) replayed on
    /// reconnect. Held as a plain `String` for the pragmatic subset; a
    /// future hardening pass can wrap it in `Zeroizing`.
    ssh_password: Option<String>,
    /// GAP-5.6 reconnect state machine. `Arc<Mutex<..>>` so the iced loop
    /// (UI reads) and the reconnect worker (writes Progress/Failed) can both
    /// touch it.
    reconnect_state: Arc<Mutex<SshState>>,
    /// Set to `true` before any user-initiated close path (manual disconnect
    /// / tab close) writes `exit\r`. The exit handler reads this to decide
    /// whether to auto-reconnect: a user-intended exit never reconnects.
    user_initiated_close: Arc<AtomicBool>,
    /// GAP-5.6: the daemon session id when this context mirrors a
    /// `puretermd`-owned `ssh -tt` child (set by the daemon paths). `None` for
    /// a direct (non-daemon) SSH context, which cannot survive GUI restart.
    session_id: Option<String>,
    /// GAP-40: compact forward summary for status-bar (`L2 R1 X`).
    forward_summary: Option<String>,
    /// GAP-40: full forward bundle (reconnect + display).
    ssh_forwards: SshForwards,
    /// GAP-41: ProxyJump chain for reconnect.
    ssh_jump_hosts: Vec<String>,
}

/// The swappable per-connection runtime inside [`SshCtx`].
#[cfg(unix)]
struct SshRuntime {
    notifier: Notifier,
    event_rx: Receiver<TerminalEvent>,
    /// Handle to the `PtyEventLoop` thread, type-erased so we can name it
    /// without importing alacritty's private `State`. Joined on `reattach`.
    io_thread: Option<ThreadJoiner>,
    /// Non-blocking stderr pipe from the ssh child process; read for error
    /// messages after a quick connection failure.
    stderr_file: Option<File>,
}

/// Owns a `JoinHandle` with its type parameter erased, so [`SshCtx`] does not
/// have to name alacritty_terminal's private `State` type. [`Self::join`]
/// waits for the thread; dropping detaches it (matching the previous
/// `let _io_thread = pty_loop.spawn()` behaviour).
#[cfg(unix)]
struct ThreadJoiner(Option<Box<dyn FnOnce() + Send + 'static>>);

#[cfg(unix)]
impl ThreadJoiner {
    fn new<T: Send + 'static>(handle: std::thread::JoinHandle<T>) -> Self {
        Self(Some(Box::new(move || {
            let _ = handle.join();
        })))
    }

    /// Wait for the wrapped thread to finish (no-op after the first call).
    fn join(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

/// P2 of GAP-5.7/5.8/5.9: TerminalContext is now an enum with two
/// variants. The public API surface is preserved so call sites in
/// `iced_app.rs` and `terminal_shader.rs` only need to be retargeted at
/// the new `new_local` / `new_ssh` constructors; method semantics on
/// the enum are the same.
pub enum TerminalContext {
    Local(LocalCtx),
    #[cfg(unix)]
    Ssh(SshCtx),
}

/// GAP-5.1/5.6 shared wiring: build a `LocalCtx` that mirrors a daemon PTY
/// over `data_path`. Used by both the create path (fresh session, after
/// `create_local`) and the attach path (recovery of an existing session, after
/// `attach_existing`). `child_pid` is 0 — the child is owned by the daemon.
#[cfg(unix)]
fn wire_local_daemon(
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
fn wire_ssh_daemon(
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
        user_initiated_close: Arc::new(AtomicBool::new(false)),
        session_id: Some(session_id),
        forward_summary: profile.forwards().summary(),
        ssh_forwards: profile.forwards(),
        ssh_jump_hosts: profile.jump_hosts.clone(),
    })
}

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
    pub fn poll_terminal_event(&self) -> Option<TerminalEvent> {
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
            ctx.user_initiated_close.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// True iff a user-initiated close was flagged for this SSH pane.
    #[cfg(unix)]
    pub fn is_user_initiated_close(&self) -> bool {
        match self {
            TerminalContext::Local(_) => false,
            TerminalContext::Ssh(ctx) => ctx.user_initiated_close.load(std::sync::atomic::Ordering::SeqCst),
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
        let pty_loop = PtyEventLoop::new(
            Arc::clone(&ctx.terminal),
            event_listener,
            ssh_pty,
            false,
            false,
        )?;
        let loop_tx = pty_loop.channel();
        let io_thread = ThreadJoiner::new(pty_loop.spawn());

        // 4. Swap the swappable runtime state.
        rt.notifier = Notifier(loop_tx);
        rt.event_rx = event_rx;
        rt.io_thread = Some(io_thread);
        rt.stderr_file = stderr;
        Ok(())
    }
}

/// GAP-5.6: inject a persistent "reconnected" separator line into the terminal
/// grid via the VT parser — the same path PTY output takes — so it survives in
/// scrollback as a boundary between the old session's output and the new one.
/// Used on the `Authenticating → Connected` transition after a successful
/// reconnect. [`TerminalContext::write_to_pty`] is the wrong tool here: it
/// forwards bytes to the remote shell's stdin, not the local grid.
pub(crate) fn inject_reconnect_marker_into(
    term: &Arc<FairMutex<Term<ChannelEventListener>>>,
    host: &str,
) {
    // Bright-black (SGR 90) line wrapped in CR-LF so it lands on its own row
    // regardless of the cursor column when the reconnect completed.
    let line = format!("\r\n\x1b[90m── Reconnected to {host} ──\x1b[0m\r\n");
    let mut parser: ansi::Processor = ansi::Processor::new();
    let mut guard = term.lock_unfair();
    parser.advance(&mut *guard, line.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::index::{Column, Point};
    use alacritty_terminal::sync::FairMutex;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::{Config, Term};
    use alacritty_terminal::vte::ansi;
    use crossbeam_channel::bounded;
    use std::sync::Arc;

    fn blank_term() -> Arc<FairMutex<Term<ChannelEventListener>>> {
        let (tx, _rx) = bounded::<TerminalEvent>(16);
        let size = TermSize::new(80, 24);
        let term = Term::new(Config::default(), &size, ChannelEventListener::new(tx));
        Arc::new(FairMutex::new(term))
    }

    fn grid_string(term: &Arc<FairMutex<Term<ChannelEventListener>>>) -> String {
        let t = term.lock();
        let start = Point::new(t.topmost_line(), Column(0));
        let end = Point::new(t.bottommost_line(), t.last_column());
        t.bounds_to_string(start, end)
    }

    #[test]
    fn reconnect_marker_writes_into_grid() {
        let term = blank_term();
        inject_reconnect_marker_into(&term, "example.com");
        let s = grid_string(&term);
        assert!(s.contains("Reconnected"), "grid missing 'Reconnected': {s:?}");
        assert!(s.contains("example.com"), "grid missing host: {s:?}");
    }

    #[test]
    fn reconnect_marker_preserves_prior_content() {
        // GAP-5.6: the marker is injected into the SAME terminal Arc (scrollback
        // preserved on reattach), so prior output must survive the injection.
        let term = blank_term();
        {
            let mut parser: ansi::Processor = ansi::Processor::new();
            let mut t = term.lock();
            parser.advance(&mut *t, b"prior scrollback line\r\n");
        }
        inject_reconnect_marker_into(&term, "host");
        let s = grid_string(&term);
        assert!(s.contains("prior scrollback line"), "prior content lost: {s:?}");
        assert!(s.contains("Reconnected"), "marker missing: {s:?}");
    }

    #[test]
    fn gap13_title_event_forwards_through_bridge() {
        // GAP-1.3: OSC 0/2 set + CSI 23t pop both route through upstream
        // `Term::set_title` → `Event::Title`; the bridge must forward it
        // (previously dropped by the `_ => return` catch-all).
        let (tx, rx) = bounded::<TerminalEvent>(16);
        let listener = ChannelEventListener::new(tx);
        listener.send_event(alacritty_terminal::event::Event::Title("hello".into()));
        assert_eq!(rx.try_recv(), Ok(TerminalEvent::Title("hello".into())));
    }

    #[test]
    fn gap13_reset_title_event_forwards_through_bridge() {
        let (tx, rx) = bounded::<TerminalEvent>(16);
        let listener = ChannelEventListener::new(tx);
        listener.send_event(alacritty_terminal::event::Event::ResetTitle);
        assert_eq!(rx.try_recv(), Ok(TerminalEvent::ResetTitle));
    }

    #[test]
    fn gap13_unmapped_events_still_dropped() {
        // Events we don't map must still be dropped (regression guard for the
        // catch-all `_ => return`).
        let (tx, rx) = bounded::<TerminalEvent>(16);
        let listener = ChannelEventListener::new(tx);
        listener.send_event(alacritty_terminal::event::Event::MouseCursorDirty);
        assert!(rx.try_recv().is_err(), "MouseCursorDirty should be dropped");
    }

    #[test]
    fn gap09_notification_event_forwards_through_bridge() {
        // GAP-09: OSC 9/99/777 notifications must forward through the bridge
        // (was dropped by the `_ => return` catch-all — same trap as GAP-15).
        use alacritty_terminal::event::{Notification, NotifyOccasion, NotifySource};
        let (tx, rx) = bounded::<TerminalEvent>(16);
        let listener = ChannelEventListener::new(tx);
        let n = Notification {
            title: Some("T".into()),
            body: "B".into(),
            id: None,
            occasion: NotifyOccasion::Always,
            source: NotifySource::Osc9,
        };
        listener.send_event(alacritty_terminal::event::Event::Notification(n.clone()));
        assert_eq!(rx.try_recv(), Ok(TerminalEvent::Notification(n)));
    }

    #[test]
    fn gap09_bell_event_forwards_through_bridge() {
        // GAP-09: bell (0x07) is an additional "needs attention" signal and must
        // forward (was dropped by the `_ => return` catch-all).
        let (tx, rx) = bounded::<TerminalEvent>(16);
        let listener = ChannelEventListener::new(tx);
        listener.send_event(alacritty_terminal::event::Event::Bell);
        assert_eq!(rx.try_recv(), Ok(TerminalEvent::Bell));
    }

    /// GAP-2.1 DoD(2026-06-29 review):切走 workspace 后,后台 PTY 持续产出的输出
    /// 必须累进进其 grid,切回来即见全部 scrollback。本仓既有测试只断言 Arc 身份存活
    /// (`switch_workspace_preserves_inactive_workspace_surface`,较弱);本测试直接锁
    /// 保活机制本身——字节进 grid **不依赖 iced wakeup 轮询**。模拟后台 pane:用
    /// `parser.advance` 把字节喂进 grid(如同 alacritty IO 线程 `term.lock().advance()`,
    /// 与 iced 主循环解耦),而事件 channel(`_rx`)从不被 drain(wakeup 循环此刻不
    /// 轮询该 pane),断言 grid 仍累进。
    #[test]
    fn gap21_background_grid_accumulates_without_event_polling() {
        let term = blank_term();

        // 「切走」前:喂一行。
        {
            let mut parser: ansi::Processor = ansi::Processor::new();
            let mut t = term.lock();
            parser.advance(&mut *t, b"line-before-switch\r\n");
        }
        let before = grid_string(&term);
        assert!(before.contains("line-before-switch"), "初始内容未进 grid: {before:?}");

        // 「切走后」:模拟后台 PTY 持续产出——字节直接进 grid(IO 线程独立喂养);
        // 此刻不 drain 事件 channel(不调 poll_terminal_event),正如 iced 的 16ms
        // wakeup 循环只轮询活跃窗口·活跃 workspace。
        {
            let mut parser: ansi::Processor = ansi::Processor::new();
            let mut t = term.lock();
            parser.advance(&mut *t, b"produced-while-backgrounded\r\n");
        }

        // 「切回来」:grid 既含切换前内容,也含后台期间产出的新内容(未丢)。
        let after = grid_string(&term);
        assert!(
            after.contains("line-before-switch"),
            "切换前内容丢失(DoD): {after:?}"
        );
        assert!(
            after.contains("produced-while-backgrounded"),
            "后台期间产出未进 grid(DoD 失守): {after:?}"
        );
    }
}
