//! SSH tunnel operations: start, list, stop, provide_auth.

use std::io;
use std::path::Path;

use super::rpc;

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
    forward_agent: bool,
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
    // GAP-5.4: request ssh-agent forwarding + agent-key auth on the tunnel.
    if forward_agent {
        params["forward_agent"] = serde_json::Value::Bool(true);
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