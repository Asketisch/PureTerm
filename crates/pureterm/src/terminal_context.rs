use std::cell::RefCell;
use std::fs::File;
use std::io;
use std::io::Read as _;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(unix)]
use alacritty_terminal::tty::EventedPty as _;
use alacritty_terminal::event::{EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty::{self, Options as PtyOptions};
use crossbeam_channel::{bounded, Receiver, Sender};

#[cfg(unix)]
use crate::ssh::config::SshProfile;
#[cfg(unix)]
use crate::ssh::pty::SshPty;

/// Terminal event types sent from PTY thread to Iced.
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    Wakeup,
    Exit,
    CwdChanged(String),
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
}

/// SSH PTY context: owns a remote ssh -tt child.
#[cfg(unix)]
struct SshCtx {
    terminal: Arc<FairMutex<Term<ChannelEventListener>>>,
    notifier: Notifier,
    event_rx: Receiver<TerminalEvent>,
    size: TermSize,
    /// Human-readable profile (kept for the tab title in P3).
    profile_display: String,
    /// Optional credentials used to authenticate (decrypted password).
    credentials: Option<zeroize::Zeroizing<Vec<u8>>>,
    /// Connection key fields for pool release on close.
    ssh_host: String,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    /// Non-blocking stderr pipe from the ssh child process.
    /// Used to read error messages after a quick connection failure.
    /// Wrapped in RefCell for interior mutability (read needs &mut self).
    stderr_file: RefCell<Option<File>>,
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

impl TerminalContext {
    /// Create a new local-pty terminal context. Equivalent to the
    /// pre-P2 `TerminalContext::new()`.
    pub fn new_local() -> io::Result<Self> {
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

        let pty_options = PtyOptions::default();
        let window_size = WindowSize {
            num_lines,
            num_cols,
            cell_width,
            cell_height,
        };
        tty::setup_env();

        // --- OSC 7 CWD hook via environment (no PTY stdin injection) ---
        //
        // For zsh: set ZDOTDIR to a temp dir containing a .zshenv that
        // restores the original ZDOTDIR, sources the user's real .zshenv,
        // then loads our OSC 7 hook.  Inspired by cmux's shell integration.
        //
        // For bash: set PROMPT_COMMAND env var with the hook.
        //
        // This avoids sending bytes via Msg::Input which can block the PTY
        // IO thread on macOS .app bundles when the payload exceeds ~80 bytes.

        let osc7_func = r#"_pt_osc7() { printf '\e]7;file://%s%s\e\\' "$HOST" "$PWD"; }"#;
        let pid = std::process::id();

        // -- zsh: ZDOTDIR bootstrap --
        let zdot_dir = format!("/tmp/.pureterm-zdotdir-{}", pid);
        let _ = std::fs::create_dir_all(&zdot_dir);
        let real_zdotdir = std::env::var("ZDOTDIR")
            .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        let zshenv_content = format!(
            r#"# PureTerm zsh bootstrap — restores original ZDOTDIR and loads OSC 7 hook.
ZDOTDIR="{real_zdotdir}"
if [ -f "$ZDOTDIR/.zshenv" ]; then . "$ZDOTDIR/.zshenv"; fi
{osc7_func}
autoload -Uz add-zsh-hook 2>/dev/null && add-zsh-hook chpwd _pt_osc7
_pt_osc7
"#,
            real_zdotdir = real_zdotdir.replace('"', "\\\""),
            osc7_func = osc7_func,
        );
        let _ = std::fs::write(format!("{}/.zshenv", zdot_dir), zshenv_content);
        std::env::set_var("ZDOTDIR", &zdot_dir);

        // -- bash: PROMPT_COMMAND env var --
        // The function def + initial call must be in PROMPT_COMMAND because
        // bash reads this env var on startup.
        std::env::set_var(
            "PROMPT_COMMAND",
            format!(
                "{func}; _pt_osc7; __pt_pc=$PROMPT_COMMAND; PROMPT_COMMAND=\"_pt_osc7${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}\"",
                func = osc7_func,
            ),
        );

        let pty = tty::new(&pty_options, window_size, 0)?;

        // Restore ZDOTDIR so subsequent PTY spawns aren't affected.
        std::env::set_var("ZDOTDIR", &real_zdotdir);

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
        }))
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
        credentials: Option<zeroize::Zeroizing<Vec<u8>>>,
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

        // Hand the concrete SshPty to PtyEventLoop. P3 introduces a
        // separate `Arc<Mutex<SshPty>>` for graceful_close; for the P2
        // internal refactor, the loop owns the pty for its full lifetime.
        let pty_loop = PtyEventLoop::new(
            Arc::clone(&terminal),
            event_listener,
            ssh_pty,
            false,
            false,
        )?;

        let loop_tx = pty_loop.channel();
        let _io_thread = pty_loop.spawn();

        // Note: SshCtx keeps a `Box<dyn EventedPty + Send>` mirror, but
        // the PtyEventLoop is what drives it. The mirror here is for
        // future resize/close paths (P3). socket_path is recorded in the
        // manager's pool and also stored here for release-on-close.
        let _ = socket_path;

        // P2 note: the PtyEventLoop owns the actual SshPty for now. P3
        // will introduce a richer wrapper struct (with Arc<Mutex<SshPty>>)
        // once graceful_close is wired into the Tab close path; for the
        // P2 internal-refactor milestone we just track the profile name
        // and credentials.
        Ok(TerminalContext::Ssh(SshCtx {
            terminal,
            notifier: Notifier(loop_tx),
            event_rx,
            size,
            profile_display: profile.name.clone(),
            credentials,
            ssh_host: profile.host.clone(),
            ssh_user: profile.user.clone(),
            ssh_port: profile.port,
            stderr_file: RefCell::new(stderr),
        }))
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
            TerminalContext::Ssh(ctx) => ctx.notifier.notify(bytes.to_vec()),
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
                let _ = ctx.notifier.0.send(Msg::Resize(window_size));
            },
        }
    }

    /// Event receiver for the iced loop to poll.
    pub fn event_rx(&self) -> &Receiver<TerminalEvent> {
        match self {
            TerminalContext::Local(ctx) => &ctx.event_rx,
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => &ctx.event_rx,
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
                let mut file_ref = ctx.stderr_file.borrow_mut();
                let file = file_ref.as_mut()?;
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
}
