//! T-core-tmux-cc: the `Surface` variant that holds a `tmux -CC` child.
//!
//! v1: a struct with the metadata the GUI needs to spawn + control a
//! control-mode tmux child, plus the `SurfaceBackend` enum that lets
//! [`crate::split_tree::Surface`] express "this is a tmux -CC pane".

use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use parking_lot::Mutex;

use super::envelope::TmuxEnvelope;

/// The running control-mode tmux child. v1 holds:
/// - the pane id tmux assigned us (so we can route `%output` back to
///   the right `alacritty_terminal::Term`);
/// - the session id (`$N`) so the GUI can show it in the tab title;
/// - the latest window snapshot from `%windows … %end`;
/// - the kill flag the GUI flips on detach.
///
/// The actual spawn / stdio thread / SIGHUP cleanup is in
/// [`super::pty`] (Unix-only in v1).
#[derive(Debug)]
pub struct TmuxControlPty {
    /// tmux session id (`$0`, `$1`, …).
    pub session_id: String,
    /// Initial pane id tmux created for the `-CC` attach. Subsequent
    /// panes come back via `%window-add` / `%output`.
    pub pane_id: u32,
    /// Latest window snapshot: `window_id → window_index`.
    pub windows: Mutex<Vec<(String, u32)>>,
    /// Replay payloads received in the first `%begin … %end` block.
    /// The renderer uses these to seed the scrollback.
    pub replay: Mutex<Vec<u8>>,
    /// Process handle (Unix pid, used for SIGHUP on detach).
    #[cfg(unix)]
    pub pid: u32,
    /// The latest grid size the GUI asked for. Sent as `resize-pane`
    /// on every window resize.
    pub last_size: Mutex<WindowSize>,
}

impl TmuxControlPty {
    /// Construct a new `TmuxControlPty` from the metadata tmux printed
    /// during attach. `replay` is the first `%output` payload, if any.
    #[cfg(unix)]
    pub fn new(session_id: String, pane_id: u32, pid: u32) -> Self {
        Self {
            session_id,
            pane_id,
            windows: Mutex::new(Vec::new()),
            replay: Mutex::new(Vec::new()),
            pid,
            last_size: Mutex::new(WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 7,
                cell_height: 14,
            }),
        }
    }

    /// Non-unix constructor (Windows fallback for v1 — the control-mode
    /// backend is Unix-only today, but the type exists so `Surface` can
    /// carry it without an extra `#[cfg]` at every match site).
    #[cfg(not(unix))]
    pub fn new(session_id: String, pane_id: u32) -> Self {
        Self {
            session_id,
            pane_id,
            windows: Mutex::new(Vec::new()),
            replay: Mutex::new(Vec::new()),
            last_size: Mutex::new(WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 7,
                cell_height: 14,
            }),
        }
    }

    /// Apply an incoming envelope to the local state. The renderer
    /// queries the state after the GUI has run a batch of envelopes.
    pub fn apply_envelope(&self, env: &TmuxEnvelope) {
        match env {
            TmuxEnvelope::Output { pane_id, bytes } if *pane_id == self.pane_id => {
                self.replay.lock().extend_from_slice(bytes);
            },
            TmuxEnvelope::Begin { pane_id, .. } if *pane_id == self.pane_id => {
                // Begin a new replay block; clear the buffer.
                self.replay.lock().clear();
            },
            TmuxEnvelope::End => {
                // Frame closed — the GUI can flush self.replay to the
                // terminal grid in a single pass.
            },
            TmuxEnvelope::SessionChanged { session_id, .. } => {
                // Replace the local session id (reattach target moved).
                // `self.session_id` is a `String` (not a `Mutex<String>`)
                // because the GUI only writes it once at attach time;
                // any race here is a known limitation logged in v1.
                let mut cur = self.session_id.clone();
                cur = session_id.clone();
                // We can't write to `self.session_id` (no `&mut self`),
                // so this is a no-op in v1; the GUI is expected to spawn
                // a fresh `TmuxControlPty` on session change.
                let _ = cur;
            },
            _ => {},
        }
    }
}

/// Backend kind for a `Surface`. The enum is `cfg`-gated so the legacy
/// `Local` / `Ssh` / `Serial` variants don't pull Unix-only code into
/// the Windows build; the new `TmuxControl` variant is also Unix-only
/// because the only backend impl in v1 is the Unix one.
#[derive(Debug, Clone)]
pub enum SurfaceBackend {
    /// Local PTY.
    Local,
    /// SSH PTY (Unix only — Windows path lives in
    /// `pureterm-ssh::windows`).
    #[cfg(unix)]
    Ssh,
    /// Serial / telnet (Unix only).
    #[cfg(unix)]
    Serial,
    /// tmux control mode child.
    #[cfg(unix)]
    TmuxControl(Arc<TmuxControlPty>),
}
