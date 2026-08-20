//! 会话生命周期操作:创建、附加、列出、杀掉、休眠、恢复。

use std::io;
use std::path::{Path, PathBuf};

use super::rpc;

/// 通过守护进程创建新的本地会话,返回 `(session_id, data_path)`。
///
/// `cwd` 为 `Some` 时作为 shell 的启动
/// 目录转发给守护进程(新标签页 / 窗格因此继承当前工作目录)。
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
    // GAP-7.5:可选的显式 argv(agent 的冷恢复,例如
    // `claude --resume <id>`)+ 用于休眠资格判定的 agent_kind 标签。
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

    // 附加(attach)以获取数据路径。
    let data_path = attach_existing(socket, &sid)?;

    Ok((sid, data_path))
}

/// 通过守护进程创建新 SSH 会话(GAP-5.1 M3),返回
/// `(session_id, data_path)`。守护进程持有 `ssh -tt` 子进程,
/// 使远端 shell 在 GUI 重启后依然存活。`password`(已由 GUI 解析并解锁)
/// 经本地控制 socket 转发,
/// 用于驱动 `SSH_ASKPASS`;`None` 表示密钥 / agent 认证。
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
    // GAP-5.4:ssh-agent 转发开关(+ 可选的显式 agent socket)。
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

/// 附加到*已存在*的守护进程会话,返回其数据 socket 路径。
///
/// 创建流程(`session.create` 之后立即附加)以及
/// GAP-5.6 跨重启恢复共用该函数:后者附加到
/// 上次 GUI 运行中创建、且仍在守护进程里存活的会话。
/// 守护进程以 `SCROLLBACK` 帧向新订阅者重放会话回滚,
/// 调用方的 `Term` 与 `vte::ansi::Processor` 得以重建
/// 网格。
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

/// 列出守护进程中的所有存活会话(GAP-5.6 恢复发现),每个
/// 元素都是 `{ id, kind, child_pid, data_path }` 原始 JSON 对象,来自
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

/// 杀掉守护进程会话(SIGHUP 子进程并拆除)。GAP-5.6:用户主动
/// 关闭标签页 / 窗格会终结底层会话,因此在下次
/// 启动时不会被恢复 —— 只有退出应用 / 杀进程才会保留会话。返回
/// 守护进程的 `killed` 标志(会话已不存在时为 `false`)。
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

/// GAP-7.3:对守护进程会话的子进程发送 SIGSTOP(休眠)。PTY 保持打开,
/// 因此 `resume_session` 可用 SIGCONT 令其复活。返回守护进程的
/// `hibernated` 标志(会话未知时为 `false`)。
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

/// GAP-7.3:对已休眠的守护进程会话子进程发送 SIGCONT(恢复)。返回
/// 守护进程的 `resumed` 标志(会话未知时为 `false`)。
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
