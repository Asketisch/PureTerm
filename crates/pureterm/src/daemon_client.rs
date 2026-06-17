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
/// reachable, returns immediately. Otherwise spawns `puretermd` as a
/// detached child and polls until the socket becomes reachable (up to ~5s).
pub fn ensure_daemon() -> io::Result<PathBuf> {
    let socket = pureterm_daemon::discovery::socket_path();
    if ping(&socket) {
        return Ok(socket);
    }

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
    for attempt in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if ping(&socket) {
            return Ok(socket);
        }
    }

    Err(io::Error::new(io::ErrorKind::TimedOut, "puretermd did not become reachable"))
}

/// Create a new local session via the daemon and return `(session_id, data_path)`.
pub fn create_local(socket: &Path, cols: u16, rows: u16) -> io::Result<(String, PathBuf)> {
    let resp = rpc(
        socket,
        "session.create",
        Some(serde_json::json!({ "cols": cols, "rows": rows })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let result = resp.result.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no result"))?;
    let sid = result["session_id"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing session_id"))?
        .to_string();

    // Attach to get the data path.
    let attach_resp = rpc(
        socket,
        "session.attach",
        Some(serde_json::json!({ "session_id": sid })),
    )?;
    if let Some(err) = &attach_resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let attach_result = attach_resp.result.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no attach result"))?;
    let data_path = attach_result["data_path"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing data_path"))?;

    Ok((sid, PathBuf::from(data_path)))
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

    let attach_resp = rpc(
        socket,
        "session.attach",
        Some(serde_json::json!({ "session_id": sid })),
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

    Ok((sid, PathBuf::from(data_path)))
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
