//! SSH 隧道操作:start、list、stop、provide_auth。

use std::io;
use std::path::Path;

use super::rpc;

/// GAP-5.2:在守护进程中启动一条 russh SOCKS5 隧道。同步绑定本地监听器,
/// 然后派发异步连接,因此返回的 `tunnel_id` 立即可用——
/// 轮询 `tunnel_list` 观察 `connecting → connected`。
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
    // GAP-5.4:在隧道上请求 ssh-agent 转发 + 代理密钥认证。
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

/// GAP-5.2:列出守护进程的隧道。每个元素是 `tunnel.list` 返回的
/// 原始 `{ id, host, user, port, local_addr, status, error? }` 对象。
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

/// GAP-5.2:按 id 停止守护进程隧道。返回守护进程的 `stopped` 标志。
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

/// GAP-5.2:向停在 `need_auth` 状态的隧道提供认证密钥。
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
