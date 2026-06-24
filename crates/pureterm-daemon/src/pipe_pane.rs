//! GAP-6.6: pipe-pane — tap a session's PTY output to a shell command (tmux
//! `pipe-pane`).
//!
//! The pump thread feeds PTY bytes into an mpsc channel; a tokio task drains
//! it into the spawned command's stdin. Stopping = dropping the sender (the
//! task ends; `kill_on_drop` reaps the child). The pump is a plain `std::thread`
//! and never touches the runtime — only `UnboundedSender::send` (which never
//! blocks), so a slow/dead child can't stall PTY delivery to subscribers.

use std::io;
use std::process::Stdio;

use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::session::SessionManager;

/// Start piping a session's output to `command` (run via `sh -c`). Spawns the
/// child + a runtime task that drains the feeder channel into the child's
/// stdin. Returns the sender the pump writes PTY bytes to. The task owns the
/// `Child` (with `kill_on_drop`), so dropping the sender ends the task and
/// reaps the child.
pub fn start(rt: &tokio::runtime::Handle, command: &str) -> io::Result<UnboundedSender<Vec<u8>>> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdin = child.stdin.take();
    let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
    rt.spawn(drain(rx, stdin, child));
    Ok(tx)
}

async fn drain(
    mut rx: UnboundedReceiver<Vec<u8>>,
    mut stdin: Option<ChildStdin>,
    child: tokio::process::Child,
) {
    use tokio::io::AsyncWriteExt;
    // Hold `child` for the task's lifetime so `kill_on_drop` only fires when
    // the task ends (sender dropped → recv returns None → loop exits). Named
    // `_child` so its non-use isn't flagged while still keeping it alive.
    let _child = child;
    while let Some(chunk) = rx.recv().await {
        if let Some(s) = stdin.as_mut() {
            if s.write_all(&chunk).await.is_err() {
                break;
            }
        }
    }
    drop(stdin);
}

/// GAP-6.6: set a session's pipe (tmux `pipe-pane [command]`). `command` None
/// ⇒ clear (stop piping). Replaces any prior pipe (dropping its sender kills
/// the old child). Returns Err if the session is unknown or the spawn fails.
pub fn set(
    manager: &SessionManager,
    rt: &tokio::runtime::Handle,
    sid: &str,
    command: Option<&str>,
) -> io::Result<()> {
    let handle = manager
        .session_handle(sid)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("unknown session {sid:?}")))?;
    // Drop any prior sender first (kills its child), then start the new one
    // while the pump sees None (no partial feed to a dying child).
    let new_tx = {
        let h = handle.lock().unwrap();
        let _prev = h.pipe_feeder.lock().unwrap().take();
        match command {
            Some(cmd) => Some(start(rt, cmd)?),
            None => None,
        }
    };
    let h = handle.lock().unwrap();
    *h.pipe_feeder.lock().unwrap() = new_tx;
    Ok(())
}

/// GAP-6.6: stop piping a session (tmux `pipe-pane` with no command). No-op if
/// the session is unknown or had no pipe.
pub fn clear(manager: &SessionManager, sid: &str) {
    if let Some(handle) = manager.session_handle(sid) {
        let h = handle.lock().unwrap();
        let _ = h.pipe_feeder.lock().unwrap().take();
    }
}

/// GAP-6.6: session ids that currently have an active pipe-pane tap.
pub fn list(manager: &SessionManager) -> Vec<String> {
    manager.pipe_status()
}
