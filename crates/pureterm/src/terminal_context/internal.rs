//! 内部载体类型:`LocalCtx` / `SshCtx` / `SshRuntime` / `ThreadJoiner`。
//!
//! 这些 struct 是 [`super::TerminalContext`] 枚举两个变体的内部状态,字段
//! 都标记为 `pub(super)`,以便同模块下的构造(`construct.rs`)、接线
//! (`wiring.rs`)、操作(`ops.rs`)实现块按变体访问字段。文件外部不可见。

use std::cell::RefCell;
use std::fs::File;
use std::sync::Arc;

#[cfg(unix)]
use std::sync::atomic::AtomicBool;
#[cfg(unix)]
use std::sync::Mutex;

use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::Term;
use crossbeam_channel::Receiver;

#[cfg(unix)]
use pureterm_ssh::SshForwards;

use super::ChannelEventListener;
use alacritty_terminal::event_loop::Notifier;
#[cfg(unix)]
use crate::ssh::reconnect::SshState;

/// Local PTY context: owns the user's shell PTY.
pub(super) struct LocalCtx {
    pub(super) terminal: Arc<FairMutex<Term<ChannelEventListener>>>,
    pub(super) notifier: Notifier,
    pub(super) event_rx: Receiver<super::TerminalEvent>,
    pub(super) size: TermSize,
    pub(super) child_pid: u32,
    /// GAP-5.6: the daemon session id when this context is a thin mirror over
    /// a `puretermd`-owned PTY (set by the daemon paths). `None` for a direct
    /// local PTY (the `new_local` fallback), which cannot survive GUI restart
    /// and is therefore not persisted/restored.
    pub(super) session_id: Option<String>,
}

/// SSH PTY context: owns a remote ssh -tt child.
#[cfg(unix)]
pub(super) struct SshCtx {
    pub(super) terminal: Arc<FairMutex<Term<ChannelEventListener>>>,
    /// Swappable per-connection runtime (notifier channel + IO thread +
    /// stderr). Wrapped in `RefCell` because `TerminalContext` lives behind
    /// an `Arc` shared with the render path, so `reattach` mutates it through
    /// interior mutability.
    pub(super) runtime: RefCell<SshRuntime>,
    pub(super) size: TermSize,
    /// Human-readable profile (kept for the tab title).
    pub(super) profile_display: String,
    /// Connection key fields for pool release on close + reconnect.
    pub(super) ssh_host: String,
    pub(super) ssh_user: Option<String>,
    pub(super) ssh_port: Option<u16>,
    /// Identity file (-i) used to authenticate; replayed on reconnect.
    pub(super) ssh_key_path: Option<String>,
    /// Resolved password (CredentialStore > TOML plaintext) replayed on
    /// reconnect. Held as a plain `String` for the pragmatic subset; a
    /// future hardening pass can wrap it in `Zeroizing`.
    pub(super) ssh_password: Option<String>,
    /// GAP-5.6 reconnect state machine. `Arc<Mutex<..>>` so the iced loop
    /// (UI reads) and the reconnect worker (writes Progress/Failed) can both
    /// touch it.
    pub(super) reconnect_state: Arc<Mutex<SshState>>,
    /// Set to `true` before any user-initiated close path (manual disconnect
    /// / tab close) writes `exit\r`. The exit handler reads this to decide
    /// whether to auto-reconnect: a user-intended exit never reconnects.
    pub(super) user_initiated_close: Arc<AtomicBool>,
    /// GAP-5.6: the daemon session id when this context mirrors a
    /// `puretermd`-owned `ssh -tt` child (set by the daemon paths). `None` for
    /// a direct (non-daemon) SSH context, which cannot survive GUI restart.
    pub(super) session_id: Option<String>,
    /// GAP-40: compact forward summary for status-bar (`L2 R1 X`).
    pub(super) forward_summary: Option<String>,
    /// GAP-40: full forward bundle (reconnect + display).
    pub(super) ssh_forwards: SshForwards,
    /// GAP-41: ProxyJump chain for reconnect.
    pub(super) ssh_jump_hosts: Vec<String>,
}

/// The swappable per-connection runtime inside [`SshCtx`].
#[cfg(unix)]
pub(super) struct SshRuntime {
    pub(super) notifier: Notifier,
    pub(super) event_rx: Receiver<super::TerminalEvent>,
    /// Handle to the `PtyEventLoop` thread, type-erased so we can name it
    /// without importing alacritty's private `State`. Joined on `reattach`.
    pub(super) io_thread: Option<ThreadJoiner>,
    /// Non-blocking stderr pipe from the ssh child process; read for error
    /// messages after a quick connection failure.
    pub(super) stderr_file: Option<File>,
}

/// Owns a `JoinHandle` with its type parameter erased, so [`SshCtx`] does not
/// have to name alacritty_terminal's private `State` type. [`Self::join`]
/// waits for the thread; dropping detaches it (matching the previous
/// `let _io_thread = pty_loop.spawn()` behaviour).
#[cfg(unix)]
pub(super) struct ThreadJoiner(pub(super) Option<Box<dyn FnOnce() + Send + 'static>>);

#[cfg(unix)]
impl ThreadJoiner {
    pub(super) fn new<T: Send + 'static>(handle: std::thread::JoinHandle<T>) -> Self {
        Self(Some(Box::new(move || {
            let _ = handle.join();
        })))
    }

    /// Wait for the wrapped thread to finish (no-op after the first call).
    pub(super) fn join(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}
