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

#[cfg(test)]
mod tests {
    use super::*;

    /// The core pipe-pane contract: bytes pushed onto the feeder channel reach
    /// the spawned command's stdin. `start` is the standalone entry (no
    /// `SessionManager` needed), so this exercises the pump→drain→child path
    /// directly — the part the dispatch-level `set`/`clear` tests don't cover.
    #[tokio::test]
    async fn start_feeds_bytes_to_child_stdin() {
        let out =
            std::env::temp_dir().join(format!("pt_pipe_test_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let rt = tokio::runtime::Handle::current();
        // `dd bs=1` writes each byte via its own write() syscall (no block
        // buffering), so the payload is committed to the file as soon as it
        // arrives — independent of the child exiting or being reaped. (`cat`
        // would block-buffer to a regular file, and the drain task's
        // `kill_on_drop` reaps the child immediately after closing its stdin,
        // racing — and losing to — `cat`'s EOF flush.)
        let cmd = format!("dd bs=1 of={} 2>/dev/null", out.display());
        let tx = start(&rt, &cmd).expect("start pipe");
        tx.send(b"payload-bytes".to_vec()).expect("send chunk");

        let mut got = String::new();
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(&out) {
                if s.contains("payload-bytes") {
                    got = s;
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        drop(tx); // stop the pipe (drain task ends, child reaped)
        let _ = std::fs::remove_file(&out);
        assert!(got.contains("payload-bytes"), "child stdin never received bytes: {got:?}");
    }
}
