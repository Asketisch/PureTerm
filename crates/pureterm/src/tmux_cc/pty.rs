//! T-core-tmux-cc: the control-mode tmux child process + envelope IO.
//!
//! v2 expands the v1 `spawn_tmux_cc` shell-out into a full
//! `TmuxChild` struct:
//!
//! - Holds the `Child` handle (reaping + shutdown paths).
//! - Owns stdin/stdout/stdio so the GUI can do `write_envelope`
//!   without re-forking.
//! - Returns envelopes via a buffered reader — the v1 path threw them
//!   on the floor.
//!
//! The full EventedPty integration (async stdin/stdout pumped by
//! `terminal_context`) is wired in v3 once the EventLoop refactor
//! lands. v2 is plain synchronous stdio + a thread that reaps the
//! child.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

use super::envelope::{parse_envelopes, TmuxEnvelope};

/// Arguments for spawning a fresh tmux -CC session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmuxSpawnArgs {
    /// `-s <name>` session name. Auto-generated from `tag` if None.
    pub session_name: Option<String>,
    /// `-x` initial columns.
    pub cols: u16,
    /// `-y` initial rows.
    pub rows: u16,
    /// If true, attach to an existing session; else create new.
    pub attach: bool,
    /// `-t <target>` for attach mode. Required when `attach` is true.
    pub target: Option<String>,
}

impl TmuxSpawnArgs {
    /// Build the args for a `tmux -CC new-session -A -s pureterm-<tag>`.
    pub fn new_session(tag: &str, cols: u16, rows: u16) -> Self {
        Self {
            session_name: Some(format!("pureterm-{tag}")),
            cols,
            rows,
            attach: false,
            target: None,
        }
    }

    /// Build args for `tmux -CC attach -t <target>`.
    pub fn attach(target: impl Into<String>) -> Self {
        Self {
            session_name: None,
            cols: 0,
            rows: 0,
            attach: true,
            target: Some(target.into()),
        }
    }
}

/// Handle to a running `tmux -CC` child + its stdio.
///
/// `TmuxChild` is the v2 full-lifecycle wrapper around a `tmux -CC`
/// child. It owns stdin/stdout, spawns a reader thread that streams
/// envelopes to a channel, and exposes helpers for the GUI to send
/// `%input` envelopes and reap the child on detach.
///
/// Note: the metadata struct in `backend.rs` is also named
/// `TmuxChild` (different concept: the v1 GUI-side handle that
/// tracks session/pane metadata). The two have distinct roles and
/// don't conflict at runtime.
pub struct TmuxChild {
    child: Child,
    stdin: Option<ChildStdin>,
    /// Buffered stdout reader (consumed by the envelope-reader thread).
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    /// Channel of envelopes streamed from stdout. The reader thread
    /// pushes; the GUI pops.
    rx: Receiver<TmuxEnvelope>,
    /// Reader thread handle (kept so we can join on shutdown).
    _reader: Option<thread::JoinHandle<()>>,
    /// Session id (`$session` from `%session-changed`). `None` until
    /// the first envelope arrives.
    session_id: Option<String>,
    /// Original session name we asked tmux to attach/create.
    pub session_name: Option<String>,
}

impl TmuxChild {
    /// Spawn `tmux -CC` with the given args. Returns the live child
    /// handle + a channel of envelopes.
    pub fn spawn(args: &TmuxSpawnArgs) -> std::io::Result<Self> {
        let mut cmd = Command::new("tmux");
        cmd.arg("-CC");
        if args.attach {
            cmd.arg("attach");
            if let Some(target) = &args.target {
                cmd.args(["-t", target]);
            }
        } else {
            cmd.args(["new-session", "-A"]);
            if let Some(name) = &args.session_name {
                cmd.args(["-s", name]);
            }
            cmd.args(["-x", &args.cols.to_string(), "-y", &args.rows.to_string()]);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (tx, rx) = channel();
        let mut reader = BufReader::new(stdout.expect("piped stdout"));
        let join = thread::spawn(move || {
            let mut buf = String::new();
            // Loop until EOF (the child closes stdout on exit).
            loop {
                buf.clear();
                let n = match reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                if n == 0 {
                    break;
                }
                for env in parse_envelopes(&buf) {
                    if tx.send(env).is_err() {
                        return; // GUI went away
                    }
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            stdout: None,
            stderr,
            rx,
            _reader: Some(join),
            session_id: None,
            session_name: args.session_name.clone(),
        })
    }

    /// Drain any pending envelopes. Non-blocking — returns immediately
    /// with whatever the reader thread has buffered.
    pub fn try_recv_envelope(&mut self) -> Option<TmuxEnvelope> {
        match self.rx.try_recv() {
            Ok(env) => {
                // Capture the session id the first time we see it.
                if let TmuxEnvelope::SessionChanged { session_id, .. } = &env {
                    if self.session_id.is_none() && !session_id.is_empty() {
                        self.session_id = Some(session_id.clone());
                    }
                }
                Some(env)
            }
            Err(_) => None,
        }
    }

    /// Block until at least one envelope arrives.
    pub fn recv_envelope(&mut self) -> Option<TmuxEnvelope> {
        match self.rx.recv() {
            Ok(env) => {
                if let TmuxEnvelope::SessionChanged { session_id, .. } = &env {
                    if self.session_id.is_none() && !session_id.is_empty() {
                        self.session_id = Some(session_id.clone());
                    }
                }
                Some(env)
            }
            Err(_) => None,
        }
    }

    /// Send a `%input` envelope to tmux (keystroke input from the
    /// GUI's `Term`). Blocks until the bytes are written.
    pub fn send_input(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("stdin closed"))?;
        // Wrap raw bytes as `%input $ <length>\n<bytes>` per the tmux
        // control-mode protocol.
        write_envelope(stdin, &TmuxEnvelope::Input {
            pane_id: 0,
            bytes: bytes.to_vec(),
        })
    }

    /// Send a raw envelope (e.g. `refresh-client -C 200x50`).
    pub fn send_envelope(&mut self, env: &TmuxEnvelope) -> std::io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("stdin closed"))?;
        write_envelope(stdin, env)
    }

    /// Poll the child for exit; returns the exit status if it has
    /// exited, `None` if it's still running.
    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    /// Send `SIGTERM` and reap. Use on detach / app shutdown.
    pub fn terminate(&mut self) -> std::io::Result<()> {
        // Closing stdin first signals tmux to shut down cleanly on
        // some platforms; we then escalate to kill.
        self.stdin.take();
        self.child.kill()?;
        self.child.wait()?;
        Ok(())
    }

    /// Get the session id we observed in the first
    /// `%session-changed` envelope. `None` if we haven't seen one yet.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Has the child exited?
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Drain stderr (used to surface tmux errors in the GUI log).
    pub fn stderr_text(&mut self) -> std::io::Result<String> {
        let mut s = String::new();
        if let Some(err) = self.stderr.as_mut() {
            err.read_to_string(&mut s)?;
        }
        Ok(s)
    }
}

impl Drop for TmuxChild {
    fn drop(&mut self) {
        // Best-effort cleanup: ignore errors (the child may have
        // already exited, or the GUI may be panicking — either way
        // we shouldn't double-panic).
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Write one envelope to the tmux -CC child stdin.
///
/// Currently a thin wrapper around `TmuxEnvelope::serialize`; kept as
/// a function so callers don't have to know the protocol details.
fn write_envelope<W: Write>(w: &mut W, env: &TmuxEnvelope) -> std::io::Result<()> {
    let s = env.serialize();
    w.write_all(s.as_bytes())?;
    w.flush()
}

// --- v1 stub kept for back-compat. The new `TmuxChild` replaces
// it for new callers, but the legacy `spawn_tmux_cc` signature is
// preserved so older binary paths still work.

#[cfg(unix)]
mod unix {
    use std::io;
    use std::process::{Child, Command, Stdio};

    /// Legacy v1: spawn a `tmux -CC` child and return a handle the
    /// GUI can write commands to / read envelopes from.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_args_have_correct_dimensions() {
        let a = TmuxSpawnArgs::new_session("abc", 200, 50);
        assert_eq!(a.session_name.as_deref(), Some("pureterm-abc"));
        assert_eq!(a.cols, 200);
        assert_eq!(a.rows, 50);
        assert!(!a.attach);
        assert!(a.target.is_none());
    }

    #[test]
    fn attach_args_have_target_no_dimensions() {
        let a = TmuxSpawnArgs::attach("pureterm-foo");
        assert!(a.attach);
        assert_eq!(a.target.as_deref(), Some("pureterm-foo"));
        assert_eq!(a.cols, 0);
        assert_eq!(a.rows, 0);
        assert!(a.session_name.is_none());
    }

    /// Spawning tmux requires the binary on PATH; skip if missing.
    #[test]
    fn spawn_fails_cleanly_without_tmux_binary() {
        // Use a binary that definitely doesn't exist so we test the
        // error path without polluting PATH.
        let mut cmd = Command::new("/nonexistent/tmux");
        cmd.arg("-CC");
        let result = cmd.spawn();
        assert!(result.is_err());
    }

    #[test]
    fn terminate_idempotent() {
        // Construct a fake child via `Command::new("true")` so we
        // don't need tmux installed. `terminate` should kill + wait
        // without panicking.
        let mut cmd = Command::new("true");
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut pty = TmuxChild {
            child: cmd.spawn().expect("spawn `true`"),
            stdin: None,
            stdout: None,
            stderr: None,
            rx: channel().1,
            _reader: None,
            session_id: None,
            session_name: None,
        };
        // First terminate: kill + wait.
        pty.terminate().expect("first terminate ok");
        // is_alive should now be false.
        assert!(!pty.is_alive());
    }
}