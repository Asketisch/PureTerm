//! T-core-tmux-cc: the control-mode tmux child process (Unix).
//!
//! v1 surfaces a single `spawn_tmux_cc` function that returns the
//! [`std::process::Child`] handle. The actual `EventedPty` integration
//! is deferred to v2 (it requires a full refactor of the way
//! `terminal_context` builds its `EventLoop`); v1 returns the `Child`
//! so the GUI can at least `kill()` it on detach.

#[cfg(unix)]
mod unix {
    use std::io;
    use std::process::{Child, Command, Stdio};

    /// Spawn a `tmux -CC` child and return a handle the GUI can write
    /// commands to / read envelopes from. The caller is responsible
    /// for the stdio wiring (the v1 `terminal_context::new_local` path
    /// doesn't yet have a hook for the `EventedPty` we build here).
    ///
    /// v1 returns `(child, "", pid)` — the session id is filled in
    /// from the first `%session-changed` envelope by the caller.
    pub fn spawn_tmux_cc(tag: &str, cols: u16, rows: u16) -> io::Result<(Child, String, u32)> {
        let session = format!("pureterm-{tag}");
        let child = Command::new("tmux")
            .args([
                "-CC",
                "new-session",
                "-A",
                "-s",
                &session,
                "-x",
                &cols.to_string(),
                "-y",
                &rows.to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let pid = child.id();
        Ok((child, String::new(), pid))
    }
}

#[cfg(unix)]
pub use unix::spawn_tmux_cc;
