//! GUI-side daemon client: control-plane RPC + spawn-on-demand.
//!
//! The GUI never holds a real PTY fd. It talks to `puretermd` via JSON-RPC
//! over the fixed daemon socket, then connects to the per-session data socket
//! with `DaemonPty` (which implements `EventedPty`).

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use pureterm_jsonrpc::{JsonRpcRequest, JsonRpcResponse};

/// Send a one-shot JSON-RPC request to the daemon and return the response.
pub fn rpc(
    socket: &Path,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<JsonRpcResponse> {
    let stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let request = match params {
        Some(p) => JsonRpcRequest::with_params(id, method, p),
        None => JsonRpcRequest::new(id, method),
    };

    let mut json = serde_json::to_string(&request).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    json.push('\n');

    let mut stream = stream;
    stream.write_all(json.as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut buf = String::new();
    BufReader::new(&stream).read_line(&mut buf)?;
    if buf.trim().is_empty() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no response from daemon"));
    }
    serde_json::from_str::<JsonRpcResponse>(buf.trim())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Ping the daemon. Returns `true` if it is reachable.
pub fn ping(socket: &Path) -> bool {
    rpc(socket, "daemon.ping", None)
        .map(|r| r.error.is_none())
        .unwrap_or(false)
}

/// Ensure the daemon is running (spawn-on-demand).
///
/// Returns the daemon socket path on success. If the daemon is already
/// reachable, first checks whether it is a *stale* binary (older than the
/// local `puretermd`, i.e. the bundle was repackaged) and restarts it so new
/// code actually runs. Otherwise spawns `puretermd` as a detached child and
/// polls until the socket becomes reachable (up to ~5s).
pub fn ensure_daemon() -> io::Result<PathBuf> {
    let socket = pureterm_daemon::discovery::socket_path();
    if ping(&socket) {
        // Reachable, but possibly a stale binary from before a repackage.
        // Restart when safe so new code runs; a failure here is non-fatal.
        if let Err(e) = restart_if_stale(&socket) {
            log::warn!("daemon staleness check failed: {e}");
        }
        return Ok(socket);
    }
    spawn_daemon(&socket)
}

/// Spawn `puretermd` as a detached child and poll until its socket becomes
/// reachable (up to ~5s).
fn spawn_daemon(socket: &Path) -> io::Result<PathBuf> {
    // Find the puretermd binary. In a .app bundle it lives beside the main
    // binary; otherwise look on PATH.
    let puretermd = find_puretermd();
    log::info!("spawning puretermd: {}", puretermd.display());

    let mut cmd = Command::new(&puretermd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Detach from the GUI's process group so `puretermd` survives GUI exit.
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    cmd.spawn()?;

    // Poll-connect with backoff up to ~5s.
    for _attempt in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if ping(socket) {
            return Ok(socket.to_path_buf());
        }
    }

    Err(io::Error::new(io::ErrorKind::TimedOut, "puretermd did not become reachable"))
}

/// If the running daemon is a stale binary (its `exe_mtime` differs from the
/// local `puretermd` mtime, i.e. the bundle was repackaged), terminate it and
/// spawn a fresh daemon so the new code runs. No-op when the daemon is current
/// or when either mtime is unknown.
///
/// Note: a restart terminates the old daemon's PTY sessions. This is only
/// reached right after `ping`, and the mtime check guarantees it only fires
/// when the binary genuinely changed (a repackage), so losing the old sessions
/// is the intended "switch to new code" semantics.
fn restart_if_stale(socket: &Path) -> io::Result<()> {
    let local_mtime = match local_puretermd_mtime() {
        Some(m) => m,
        None => return Ok(()), // can't read local exe mtime → can't judge staleness
    };

    let status = rpc(socket, "daemon.status", None)?;
    let result = status
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no daemon.status result"))?;
    let sessions = result.get("sessions").and_then(|v| v.as_u64()).unwrap_or(0);
    let pid = result.get("pid").and_then(|v| v.as_u64());

    // Determine staleness. A new daemon always reports `exe_mtime` (even 0 if
    // it couldn't stat its own exe), so an ABSENT field means the running
    // daemon is an older binary → stale. `exe_mtime == 0` (new daemon, stat
    // failed) can't be compared reliably → treat as not-stale to avoid a
    // restart loop.
    let stale_reason = match result.get("exe_mtime") {
        None => Some("predates staleness tracking"),
        Some(v) => match v.as_u64() {
            None | Some(0) => None,
            Some(dm) if dm != local_mtime => Some("binary updated"),
            Some(_) => None,
        },
    };

    let Some(reason) = stale_reason else {
        return Ok(());
    };

    // Stale daemon — but don't destroy live sessions automatically. A repackage
    // is not consent to discard the user's running shells. Only restart when
    // nothing is at stake; otherwise log so the user can restart deliberately
    // (e.g. `pkill puretermd` then relaunch the app).
    if sessions > 0 {
        log::warn!(
            "puretermd is stale ({}) but holds {} live session(s); not auto-restarting \
             to avoid losing them. Restart manually when ready.",
            reason,
            sessions
        );
        return Ok(());
    }

    log::warn!("puretermd is stale ({}); restarting daemon", reason);
    terminate_and_respawn(socket, pid)
}

/// SIGTERM the daemon (so its flock releases), wait for it to drop the socket,
/// then spawn a fresh daemon. `pid` is the old daemon's pid from `daemon.status`.
fn terminate_and_respawn(socket: &Path, pid: Option<u64>) -> io::Result<()> {
    if let Some(pid) = pid {
        // SIGTERM → daemon exits → flock releases → fresh daemon can acquire it.
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }

    // Wait for the old daemon to release the socket/lock (up to ~5s).
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if !ping(socket) {
            break;
        }
    }

    spawn_daemon(socket).map(|_| ())
}

/// mtime (Unix epoch secs) of the `puretermd` binary the GUI would spawn, or
/// `None` if it can't be read.
fn local_puretermd_mtime() -> Option<u64> {
    let exe = find_puretermd();
    let modified = std::fs::metadata(&exe).ok()?.modified().ok()?;
    modified.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Create a new local session via the daemon and return `(session_id, data_path)`.
///
/// `cwd`, when `Some`, is forwarded to the daemon as the shell's startup
/// directory (so new tabs/panes inherit the current working directory).
pub fn create_local(
    socket: &Path,
    cols: u16,
    rows: u16,
    cwd: Option<&str>,
    argv: Option<&[String]>,
    agent_kind: Option<&str>,
) -> io::Result<(String, PathBuf)> {
    let mut params = serde_json::json!({ "cols": cols, "rows": rows });
    if let Some(c) = cwd {
        params["working_directory"] = serde_json::Value::String(c.to_string());
    }
    // GAP-7.5: optional explicit argv (cold resume of an agent, e.g.
    // `claude --resume <id>`) + agent_kind tag for hibernation eligibility.
    if let Some(argv) = argv {
        params["argv"] = serde_json::Value::Array(
            argv.iter().map(|s| serde_json::Value::String(s.clone())).collect(),
        );
    }
    if let Some(k) = agent_kind {
        params["agent_kind"] = serde_json::Value::String(k.to_string());
    }
    let resp = rpc(socket, "session.create", Some(params))?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let result = resp.result.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no result"))?;
    let sid = result["session_id"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing session_id"))?
        .to_string();

    // Attach to get the data path.
    let data_path = attach_existing(socket, &sid)?;

    Ok((sid, data_path))
}

/// Create a new SSH session via the daemon (GAP-5.1 M3) and return
/// `(session_id, data_path)`. The daemon owns the `ssh -tt` child so the
/// remote shell survives GUI restart. `password` (already resolved + unlocked
/// by the GUI) is forwarded over the local control socket to drive
/// `SSH_ASKPASS`; `None` means key/agent auth.
pub fn create_ssh(
    socket: &Path,
    host: &str,
    user: Option<&str>,
    port: Option<u16>,
    identity_file: Option<&str>,
    password: Option<&str>,
    cols: u16,
    rows: u16,
) -> io::Result<(String, PathBuf)> {
    let mut params = serde_json::json!({
        "kind": "ssh",
        "host": host,
        "cols": cols,
        "rows": rows,
    });
    if let Some(u) = user {
        params["user"] = serde_json::Value::String(u.to_string());
    }
    if let Some(p) = port {
        params["port"] = serde_json::Value::Number(p.into());
    }
    if let Some(k) = identity_file {
        params["identity_file"] = serde_json::Value::String(k.to_string());
    }
    if let Some(pw) = password {
        params["password"] = serde_json::Value::String(pw.to_string());
    }

    let resp = rpc(socket, "session.create", Some(params))?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let result = resp.result.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no result"))?;
    let sid = result["session_id"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing session_id"))?
        .to_string();

    let data_path = attach_existing(socket, &sid)?;

    Ok((sid, data_path))
}

/// Attach to an *existing* daemon session and return its data-socket path.
///
/// Shared by the create flow (which attaches right after `session.create`)
/// and by GAP-5.6 cross-restart recovery, which attaches to a session that
/// was created in a previous GUI run and is still alive in the daemon. The
/// daemon replays the session's scrollback as `SCROLLBACK` frames to the new
/// subscriber, so the caller's `Term` + `vte::ansi::Processor` rebuilds the
/// grid naturally.
pub fn attach_existing(socket: &Path, session_id: &str) -> io::Result<PathBuf> {
    let attach_resp = rpc(
        socket,
        "session.attach",
        Some(serde_json::json!({ "session_id": session_id })),
    )?;
    if let Some(err) = &attach_resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let attach_result = attach_resp
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no attach result"))?;
    let data_path = attach_result["data_path"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing data_path"))?;
    Ok(PathBuf::from(data_path))
}

/// List all live sessions in the daemon (GAP-5.6 recovery discovery). Each
/// element is the raw JSON `{ id, kind, child_pid, data_path }` object from
/// `session.list`.
pub fn list_sessions(socket: &Path) -> io::Result<Vec<serde_json::Value>> {
    let resp = rpc(socket, "session.list", None)?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let result = resp
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no result"))?;
    let sessions = result
        .get("sessions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing sessions array"))?;
    Ok(sessions.clone())
}

/// Kill a daemon session (SIGHUP child + tear down). GAP-5.6: a user-initiated
/// tab/pane close ends the underlying session so it is NOT restored on the next
/// launch — only an app quit / process kill preserves sessions. Returns the
/// daemon's `killed` flag (`false` if the session was already gone).
pub fn kill_session(socket: &Path, session_id: &str) -> io::Result<bool> {
    let resp = rpc(
        socket,
        "session.kill",
        Some(serde_json::json!({ "session_id": session_id })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    Ok(resp
        .result
        .and_then(|r| r.get("killed").and_then(|v| v.as_bool()))
        .unwrap_or(false))
}

/// GAP-7.3: SIGSTOP a daemon session's child (hibernate). The PTY stays open,
/// so `resume_session` can SIGCONT it back to life. Returns the daemon's
/// `hibernated` flag (`false` if the session was unknown).
pub fn hibernate_session(socket: &Path, session_id: &str) -> io::Result<bool> {
    let resp = rpc(
        socket,
        "agent.hibernate",
        Some(serde_json::json!({ "session_id": session_id })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    Ok(resp
        .result
        .and_then(|r| r.get("hibernated").and_then(|v| v.as_bool()))
        .unwrap_or(false))
}

/// GAP-7.3: SIGCONT a hibernated daemon session's child (resume). Returns the
/// daemon's `resumed` flag (`false` if the session was unknown).
pub fn resume_session(socket: &Path, session_id: &str) -> io::Result<bool> {
    let resp = rpc(
        socket,
        "agent.resume",
        Some(serde_json::json!({ "session_id": session_id })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    Ok(resp
        .result
        .and_then(|r| r.get("resumed").and_then(|v| v.as_bool()))
        .unwrap_or(false))
}

/// GAP-5.2: start a russh SOCKS5 tunnel in the daemon. Binds the local listener
/// synchronously then spawns the async connect, so the returned `tunnel_id` is
/// immediately usable — poll `tunnel_list` for `connecting → connected`.
pub fn tunnel_start(
    socket: &Path,
    host: &str,
    user: Option<&str>,
    port: Option<u16>,
    identity_file: Option<&str>,
    password: Option<&str>,
    local_port: u16,
    remote_dns: bool,
) -> io::Result<String> {
    let mut params = serde_json::json!({
        "host": host,
        "local_port": local_port,
        "remote_dns": remote_dns,
    });
    if let Some(u) = user {
        params["user"] = serde_json::Value::String(u.to_string());
    }
    if let Some(p) = port {
        params["port"] = serde_json::Value::Number(p.into());
    }
    if let Some(k) = identity_file {
        params["identity_file"] = serde_json::Value::String(k.to_string());
    }
    if let Some(pw) = password {
        params["password"] = serde_json::Value::String(pw.to_string());
    }
    let resp = rpc(socket, "tunnel.start", Some(params))?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    resp.result
        .and_then(|r| r.get("tunnel_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing tunnel_id"))
}

/// GAP-5.2: list daemon tunnels. Each element is the raw `{ id, host, user,
/// port, local_addr, status, error? }` object from `tunnel.list`.
pub fn tunnel_list(socket: &Path) -> io::Result<Vec<serde_json::Value>> {
    let resp = rpc(socket, "tunnel.list", None)?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let result = resp
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no result"))?;
    let tunnels = result
        .get("tunnels")
        .and_then(|v| v.as_array())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing tunnels array"))?;
    Ok(tunnels.clone())
}

/// GAP-5.2: stop a daemon tunnel by id. Returns the daemon's `stopped` flag.
pub fn tunnel_stop(socket: &Path, tunnel_id: &str) -> io::Result<bool> {
    let resp = rpc(
        socket,
        "tunnel.stop",
        Some(serde_json::json!({ "tunnel_id": tunnel_id })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    Ok(resp
        .result
        .and_then(|r| r.get("stopped").and_then(|v| v.as_bool()))
        .unwrap_or(false))
}

/// GAP-5.2: supply the auth secret to a tunnel parked in `need_auth`.
pub fn tunnel_provide_auth(socket: &Path, tunnel_id: &str, secret: &str) -> io::Result<()> {
    let resp = rpc(
        socket,
        "tunnel.provide_auth",
        Some(serde_json::json!({ "tunnel_id": tunnel_id, "secret": secret })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    Ok(())
}

/// Locate the `puretermd` binary.
fn find_puretermd() -> PathBuf {
    // 1. Same directory as the current binary (covers .app bundles and cargo build).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("puretermd");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // 2. Fall back to PATH.
    PathBuf::from("puretermd")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_puretermd_returns_path() {
        let p = find_puretermd();
        // It may or may not exist on this machine, but it should not panic.
        assert!(!p.as_os_str().is_empty());
    }

    #[test]
    fn ping_nonexistent_returns_false() {
        assert!(!ping(Path::new("/tmp/nonexistent-puretermd-test.sock")));
    }
}
