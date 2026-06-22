//! JSON-RPC 2.0 control plane for `puretermd`.
//!
//! A one-shot Unix-socket server (one JSON-RPC request/response per
//! connection, mirroring the GUI IPC listener) exposing `daemon.*` and
//! `session.*` methods. The data plane (streaming PTY bytes) is the separate
//! per-session framed socket returned by `session.attach`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use pureterm_jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, ERROR_INVALID_PARAMS};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use crate::session::SessionManager;

/// Shared control-plane state.
#[derive(Clone)]
pub struct ControlState {
    pub manager: SessionManager,
    pub started: Instant,
    /// mtime (Unix epoch secs) of the `puretermd` executable captured at
    /// startup. Lets the GUI detect a stale daemon (older binary still
    /// running after a repackage) and restart it so new code actually runs.
    /// 0 if the exe path/mtime couldn't be read.
    pub exe_mtime: u64,
}

impl ControlState {
    pub fn new(manager: SessionManager) -> Self {
        let exe_mtime = std::env::current_exe()
            .and_then(|p| p.metadata())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self { manager, started: Instant::now(), exe_mtime }
    }
}

/// Bind the control socket and serve until the runtime is cancelled.
pub async fn serve(socket_path: PathBuf, state: Arc<ControlState>) -> std::io::Result<()> {
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    let listener = std::os::unix::net::UnixListener::bind(&socket_path)?;
    listener.set_nonblocking(true)?;
    crate::discovery::restrict_socket(&socket_path); // GAP-5.1 M5: owner-only
    let listener = UnixListener::from_std(listener)?;
    log::info!("puretermd control socket: {}", socket_path.display());

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = state.clone();
                tokio::spawn(handle_connection(stream, state));
            }
            Err(e) => {
                log::error!("control accept error: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

async fn handle_connection(stream: tokio::net::UnixStream, state: Arc<ControlState>) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    match reader.read_line(&mut line).await {
        Ok(0) | Err(_) => return,
        _ => {}
    }

    let request = match JsonRpcRequest::from_line(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            let resp = JsonRpcResponse {
                jsonrpc: pureterm_jsonrpc::JSONRPC_VERSION.into(),
                id: 0,
                result: None,
                error: Some(e),
            };
            let _ = write_half.write_all(resp.to_line().as_bytes()).await;
            return;
        }
    };

    let response = dispatch(&request, &state);
    let _ = write_half.write_all(response.to_line().as_bytes()).await;
}

/// Route a JSON-RPC request to its handler.
pub fn dispatch(request: &JsonRpcRequest, state: &ControlState) -> JsonRpcResponse {
    let id = request.id;
    let result: Result<serde_json::Value, JsonRpcError> = match request.method.as_str() {
        "daemon.ping" => Ok(serde_json::json!({"ok": true})),
        "daemon.status" => Ok(serde_json::json!({
            "pid": std::process::id(),
            "uptime_s": state.started.elapsed().as_secs(),
            "sessions": state.manager.list().len(),
            "exe_mtime": state.exe_mtime,
        })),
        "session.create" => {
            let cols = uparam(request, "cols").unwrap_or(80);
            let rows = uparam(request, "rows").unwrap_or(24);
            let kind = sparam(request, "kind").unwrap_or_else(|| "local".to_string());
            let sid = if kind == "ssh" {
                // GAP-5.1 M3: daemon-owned SSH PTY. `password` is resolved + unlocked
                // by the GUI and passed over the local control socket; it drives
                // SSH_ASKPASS. None => key/agent auth or interactive prompt.
                let host = match sparam(request, "host") {
                    Some(h) => h,
                    None => {
                        return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing host");
                    }
                };
                let user = sparam(request, "user");
                let port = uparam(request, "port");
                let identity_file = sparam(request, "identity_file");
                let password = sparam(request, "password");
                state.manager.create_ssh(
                    &host,
                    user.as_deref(),
                    port,
                    identity_file.as_deref(),
                    password.as_deref(),
                    cols,
                    rows,
                )
            } else {
                let cwd = sparam(request, "working_directory").map(PathBuf::from);
                // GAP-7.5: optional explicit argv (cold resume of an agent) +
                // agent_kind tag for hibernation eligibility (GAP-7.3).
                let argv = aparam(request, "argv");
                let agent_kind = sparam(request, "agent_kind");
                state.manager.create_local(cols, rows, cwd, argv, agent_kind)
            };
            match sid {
                Ok(s) => Ok(serde_json::json!({ "session_id": s })),
                Err(e) => Err(method_err(id, ERROR_INVALID_PARAMS, format!("create failed: {e}"))),
            }
        }
        "session.list" => Ok(serde_json::json!({ "sessions": state.manager.list() })),
        "session.attach" => {
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => {
                    return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id");
                }
            };
            match state.manager.data_path(&sid) {
                Some(path) => {
                    Ok(serde_json::json!({ "session_id": sid, "data_path": path.to_string_lossy() }))
                }
                None => Err(method_err(id, ERROR_INVALID_PARAMS, "unknown session_id".to_string())),
            }
        }
        "session.kill" => {
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => {
                    return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id");
                }
            };
            Ok(serde_json::json!({ "killed": state.manager.kill(&sid) }))
        }
        // ── agent.* (GAP-7.3 hibernation control plane) ───────────────
        "agent.hibernate" => {
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => {
                    return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id");
                }
            };
            Ok(serde_json::json!({ "hibernated": state.manager.hibernate(&sid) }))
        }
        "agent.resume" => {
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => {
                    return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id");
                }
            };
            Ok(serde_json::json!({ "resumed": state.manager.resume(&sid) }))
        }
        "agent.list_hibernated" => {
            let hib = state
                .manager
                .list()
                .into_iter()
                .filter(|s| s.hibernated)
                .collect::<Vec<_>>();
            Ok(serde_json::json!({ "sessions": hib }))
        }
        other => {
            return JsonRpcResponse::method_not_found(id, other);
        }
    };

    match result {
        Ok(value) => JsonRpcResponse::ok(id, value),
        // `method_err` already built an error carrying the id; reuse it.
        Err(e) => JsonRpcResponse {
            jsonrpc: pureterm_jsonrpc::JSONRPC_VERSION.into(),
            id,
            result: None,
            error: Some(JsonRpcError { code: e.code, message: e.message, data: None }),
        },
    }
}

/// Build an error (the `id` field is unused here; dispatch sets it on the way out).
fn method_err(_id: u64, code: i64, message: String) -> JsonRpcError {
    JsonRpcError { code, message, data: None }
}

fn uparam(request: &JsonRpcRequest, key: &str) -> Option<u16> {
    request.params.as_ref()?.get(key)?.as_u64().map(|n| n as u16)
}

fn sparam(request: &JsonRpcRequest, key: &str) -> Option<String> {
    request.params.as_ref()?.get(key)?.as_str().map(|s| s.to_string())
}

/// GAP-7.5: parse a JSON array of strings param (e.g. `argv` for cold resume
/// of an agent via `claude --resume <id>`). Non-string elements are dropped.
fn aparam(request: &JsonRpcRequest, key: &str) -> Option<Vec<String>> {
    request
        .params
        .as_ref()?
        .get(key)?
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ControlState {
        ControlState::new(SessionManager::default())
    }

    #[test]
    fn ping_ok() {
        let req = JsonRpcRequest::new(1, "daemon.ping");
        let resp = dispatch(&req, &state());
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["ok"], true);
    }

    #[test]
    fn unknown_method() {
        let req = JsonRpcRequest::new(2, "nope.nope");
        let resp = dispatch(&req, &state());
        let err = resp.error.expect("error");
        assert_eq!(err.code, pureterm_jsonrpc::ERROR_METHOD_NOT_FOUND);
    }

    #[test]
    fn attach_unknown_session_errors() {
        let req = JsonRpcRequest::with_params(
            3,
            "session.attach",
            serde_json::json!({ "session_id": "does-not-exist" }),
        );
        let resp = dispatch(&req, &state());
        let err = resp.error.expect("error");
        assert_eq!(err.code, ERROR_INVALID_PARAMS);
    }

    #[test]
    fn list_empty() {
        let req = JsonRpcRequest::new(4, "session.list");
        let resp = dispatch(&req, &state());
        let result = resp.result.unwrap();
        let sessions = result["sessions"].as_array().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn create_then_attach_round_trips_path() {
        let st = state();
        // create may fail in environments without a usable shell; only assert
        // when it succeeds.
        let create = dispatch(&JsonRpcRequest::with_params(5, "session.create", serde_json::json!({"cols":80,"rows":24})), &st);
        if create.error.is_none() {
            let sid = create.result.unwrap()["session_id"].as_str().unwrap().to_string();
            let attach = dispatch(
                &JsonRpcRequest::with_params(6, "session.attach", serde_json::json!({"session_id": sid})),
                &st,
            );
            assert!(attach.error.is_none());
            assert!(attach.result.unwrap()["data_path"].as_str().is_some());
        }
    }
}
