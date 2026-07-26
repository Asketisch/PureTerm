//! Session lifecycle operations: create, attach, list, kill, hibernate, resume.

use std::io;
use std::path::{Path, PathBuf};

use super::rpc;

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
    smallest_screen_wins: bool,
) -> io::Result<(String, PathBuf)> {
    let mut params = serde_json::json!({
        "cols": cols,
        "rows": rows,
        "smallest_screen_wins": smallest_screen_wins,
    });
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
    forward_agent: bool,
    agent_sock_override: Option<&str>,
    forwards: pureterm_ssh::SshForwards,
    smallest_screen_wins: bool,
) -> io::Result<(String, PathBuf)> {
    let mut params = serde_json::json!({
        "kind": "ssh",
        "host": host,
        "cols": cols,
        "rows": rows,
        "smallest_screen_wins": smallest_screen_wins,
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
    // GAP-5.4: ssh-agent forwarding toggle (+ optional explicit agent socket).
    if forward_agent {
        params["forward_agent"] = serde_json::Value::Bool(true);
    }
    if let Some(s) = agent_sock_override {
        params["agent_sock"] = serde_json::Value::String(s.to_string());
    }
    if !forwards.local.is_empty() {
        params["local_forwards"] = serde_json::json!(forwards.local);
    }
    if !forwards.remote.is_empty() {
        params["remote_forwards"] = serde_json::json!(forwards.remote);
    }
    if forwards.x11 {
        params["forward_x11"] = serde_json::Value::Bool(true);
    }
    if forwards.x11_trusted {
        params["forward_x11_trusted"] = serde_json::Value::Bool(true);
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