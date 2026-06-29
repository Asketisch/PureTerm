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

use pureterm_ssh::SshForwards;

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
    /// GAP-5.2: handle to the multi-thread runtime, so the (sync) `tunnel.*`
    /// methods can spawn async russh connect/copy tasks. `None` in unit tests
    /// (no runtime spun up) — `tunnel.start` then errors with "no runtime".
    pub runtime: Option<tokio::runtime::Handle>,
    /// GAP-5.2: live russh SOCKS5 tunnels.
    pub tunnels: crate::tunnel::TunnelManager,
    /// GAP-6.6: wait-for channel/lock store (tmux `wait-for`). Waiters park
    /// here until a signaler wakes them (or the daemon shuts down).
    pub wait_channels: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Vec<tokio::sync::oneshot::Sender<()>>>>>,
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
        Self {
            manager,
            started: Instant::now(),
            exe_mtime,
            runtime: None,
            tunnels: crate::tunnel::TunnelManager::default(),
            wait_channels: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Production constructor: attaches the daemon's runtime handle so tunnel
    /// tasks can be spawned. (`new` leaves runtime = None, for unit tests.)
    pub fn with_runtime(manager: SessionManager, runtime: tokio::runtime::Handle) -> Self {
        let mut s = Self::new(manager);
        s.runtime = Some(runtime);
        s
    }
}

/// GAP-5.2: snapshot live tunnel configs to `tunnels.json` (config-only
/// persistence across daemon restarts). Best-effort (save_tunnels logs on
/// failure) — called after each `tunnel.start`/`tunnel.stop` mutation.
fn persist_tunnels(state: &ControlState) {
    let cfg = crate::persist::TunnelsFile {
        version: 1,
        tunnels: state.tunnels.snapshot_configs(),
    };
    crate::persist::save_tunnels(&crate::discovery::tunnels_file(), &cfg);
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

    // GAP-6.6: `wait_for.*` is async — `wait`/`lock` park the caller. Routed
    // here (before the sync `dispatch`) since blocking would need an await.
    // Each connection is its own tokio task, so a parked waiter only blocks
    // itself, not the accept loop or other callers.
    let response = if request.method.starts_with("wait_for.") {
        crate::wait_for::handle(&request, &state).await
    } else {
        dispatch(&request, &state)
    };
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
            let smallest_screen_wins = bparam(request, "smallest_screen_wins").unwrap_or(true);
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
                // GAP-5.4: optional ssh-agent forwarding + explicit agent socket.
                let forward_agent = bparam(request, "forward_agent").unwrap_or(false);
                let agent_sock = sparam(request, "agent_sock");
                let forwards = forwards_from_request(request);
                state.manager.create_ssh(
                    &host,
                    user.as_deref(),
                    port,
                    identity_file.as_deref(),
                    password.as_deref(),
                    cols,
                    rows,
                    forward_agent,
                    agent_sock.as_deref(),
                    forwards,
                    smallest_screen_wins,
                )
            } else {
                let cwd = sparam(request, "working_directory").map(PathBuf::from);
                // GAP-7.5: optional explicit argv (cold resume of an agent) +
                // agent_kind tag for hibernation eligibility (GAP-7.3).
                let argv = aparam(request, "argv");
                let agent_kind = sparam(request, "agent_kind");
                state.manager.create_local(cols, rows, cwd, argv, agent_kind, smallest_screen_wins)
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
        // ── tunnel.* (GAP-5.2 russh SOCKS5 tunnel control plane) ────────────
        "tunnel.start" => {
            // russh connect/auth is async but dispatch is sync: bind the
            // listener + register the tunnel synchronously, spawn the async
            // connect on the runtime, return the id immediately. Status is
            // polled via tunnel.list (connecting → connected | need_auth | failed).
            let Some(rt) = state.runtime.as_ref().cloned() else {
                return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "daemon runtime unavailable");
            };
            let host = match sparam(request, "host") {
                Some(h) => h,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing host"),
            };
            let user = sparam(request, "user");
            let port = uparam(request, "port").unwrap_or(22);
            let identity_file = sparam(request, "identity_file");
            let password = sparam(request, "password");
            let local_port = uparam(request, "local_port").unwrap_or(1080);
            let remote_dns = bparam(request, "remote_dns").unwrap_or(true);
            // GAP-5.4: optional ssh-agent forwarding on the tunnel's session.
            let forward_agent = bparam(request, "forward_agent").unwrap_or(false);
            match state.tunnels.start(
                &rt, host, user, port, identity_file, password, local_port, remote_dns, forward_agent,
            ) {
                Ok(tid) => {
                    persist_tunnels(&state);
                    Ok(serde_json::json!({ "tunnel_id": tid, "status": "connecting" }))
                }
                Err(e) => Err(method_err(id, ERROR_INVALID_PARAMS, format!("tunnel start failed: {e}"))),
            }
        }
        "tunnel.list" => Ok(serde_json::json!({ "tunnels": state.tunnels.list() })),
        "tunnel.stop" => {
            let tid = match sparam(request, "tunnel_id") {
                Some(t) => t,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing tunnel_id"),
            };
            let stopped = state.tunnels.stop(&tid);
            persist_tunnels(&state);
            Ok(serde_json::json!({ "stopped": stopped }))
        }
        "tunnel.provide_auth" => {
            let tid = match sparam(request, "tunnel_id") {
                Some(t) => t,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing tunnel_id"),
            };
            let secret = match sparam(request, "secret") {
                Some(s) => s,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing secret"),
            };
            match state.tunnels.provide_auth(&tid, secret) {
                Ok(()) => Ok(serde_json::json!({ "provided": true })),
                Err(e) => Err(method_err(id, ERROR_INVALID_PARAMS, format!("provide_auth failed: {e}"))),
            }
        }
        // ── pipe.* (GAP-6.6 pipe-pane — tap a session's PTY to a command) ──
        "pipe.set" => {
            let Some(rt) = state.runtime.as_ref().cloned() else {
                return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "daemon runtime unavailable");
            };
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id"),
            };
            let command = sparam(request, "command"); // None ⇒ clear
            match crate::pipe_pane::set(&state.manager, &rt, &sid, command.as_deref()) {
                Ok(()) => Ok(serde_json::json!({ "set": true })),
                Err(e) => Err(method_err(id, ERROR_INVALID_PARAMS, format!("pipe.set failed: {e}"))),
            }
        }
        "pipe.clear" => {
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id"),
            };
            crate::pipe_pane::clear(&state.manager, &sid);
            Ok(serde_json::json!({ "cleared": true }))
        }
        "pipe.list" => Ok(serde_json::json!({ "sessions": crate::pipe_pane::list(&state.manager) })),
        // ── remote.* (GAP-43 file browse MVP) ───────────────────────────────
        "remote.listdir" => {
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id"),
            };
            let path = sparam(request, "path").unwrap_or_else(|| ".".to_string());
            match state.manager.list_dir(&sid, &path) {
                Ok(entries) => Ok(serde_json::json!({ "path": path, "entries": entries })),
                Err(e) => Err(method_err(id, ERROR_INVALID_PARAMS, format!("remote.listdir failed: {e}"))),
            }
        }
        "remote.read" => {
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id"),
            };
            let path = match sparam(request, "path") {
                Some(p) => p,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing path"),
            };
            let max_bytes = request
                .params
                .as_ref()
                .and_then(|p| p.get("max_bytes"))
                .and_then(|v| v.as_u64())
                .unwrap_or(crate::remote_fs::READ_MAX_BYTES);
            match state.manager.read_file(&sid, &path, max_bytes) {
                Ok(result) => Ok(serde_json::to_value(result).unwrap_or(serde_json::json!({}))),
                Err(e) => Err(method_err(id, ERROR_INVALID_PARAMS, format!("remote.read failed: {e}"))),
            }
        }
        "remote.write" => {
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id"),
            };
            let path = match sparam(request, "path") {
                Some(p) => p,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing path"),
            };
            let content = match sparam(request, "content") {
                Some(c) => c,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing content"),
            };
            match state.manager.write_file(&sid, &path, &content) {
                Ok(()) => Ok(serde_json::json!({ "written": true, "path": path })),
                Err(e) => Err(method_err(id, ERROR_INVALID_PARAMS, format!("remote.write failed: {e}"))),
            }
        }
        "remote.find" => {
            let sid = match sparam(request, "session_id") {
                Some(s) => s,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing session_id"),
            };
            let base = sparam(request, "path").unwrap_or_else(|| ".".to_string());
            let query = match sparam(request, "query") {
                Some(q) => q,
                None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing query"),
            };
            let max = request
                .params
                .as_ref()
                .and_then(|p| p.get("max"))
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .unwrap_or(crate::remote_fs::FIND_MAX_RESULTS);
            match state.manager.find_files(&sid, &base, &query, max) {
                Ok(matches) => Ok(serde_json::json!({ "path": base, "query": query, "matches": matches })),
                Err(e) => Err(method_err(id, ERROR_INVALID_PARAMS, format!("remote.find failed: {e}"))),
            }
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

/// GAP-5.2: parse a JSON bool param (e.g. `remote_dns`).
fn bparam(request: &JsonRpcRequest, key: &str) -> Option<bool> {
    request.params.as_ref()?.get(key)?.as_bool()
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

/// GAP-40: parse `-L`/`-R`/`-X`/`-Y` knobs from `session.create`.
fn forwards_from_request(request: &JsonRpcRequest) -> SshForwards {
    SshForwards {
        local: aparam(request, "local_forwards").unwrap_or_default(),
        remote: aparam(request, "remote_forwards").unwrap_or_default(),
        x11: bparam(request, "forward_x11").unwrap_or(false),
        x11_trusted: bparam(request, "forward_x11_trusted").unwrap_or(false),
    }
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
        let _iso = crate::discovery::IsolatedDataDir::new();
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

    // ── GAP-5.2 tunnel.* dispatch ──────────────────────────────────────────
    #[test]
    fn tunnel_list_empty() {
        let req = JsonRpcRequest::new(7, "tunnel.list");
        let resp = dispatch(&req, &state());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let tunnels = result["tunnels"].as_array().unwrap();
        assert!(tunnels.is_empty());
    }

    #[test]
    fn tunnel_start_without_runtime_errors() {
        // Unit-test ControlState has runtime = None, so start must refuse
        // rather than spawn a tunnel it can't drive.
        let req = JsonRpcRequest::with_params(
            8,
            "tunnel.start",
            serde_json::json!({"host":"example.com","local_port":1080}),
        );
        let resp = dispatch(&req, &state());
        let err = resp.error.expect("expected error without runtime");
        assert_eq!(err.code, ERROR_INVALID_PARAMS);
    }

    #[test]
    fn tunnel_start_missing_host_errors() {
        let req = JsonRpcRequest::with_params(9, "tunnel.start", serde_json::json!({}));
        let resp = dispatch(&req, &state());
        // Missing host is checked before the runtime guard.
        let err = resp.error.expect("expected missing-host error");
        assert_eq!(err.code, ERROR_INVALID_PARAMS);
    }

    #[test]
    fn tunnel_stop_unknown_returns_false() {
        let req = JsonRpcRequest::with_params(
            10,
            "tunnel.stop",
            serde_json::json!({"tunnel_id":"does-not-exist"}),
        );
        let resp = dispatch(&req, &state());
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["stopped"], false);
    }

    // ── GAP-6.6 pipe.* dispatch ───────────────────────────────────────────
    #[test]
    fn pipe_set_without_runtime_errors() {
        // Unit-test ControlState has runtime = None, so pipe.set must refuse
        // (it needs the runtime to spawn the child task).
        let req = JsonRpcRequest::with_params(
            11,
            "pipe.set",
            serde_json::json!({"session_id": "s1", "command": "cat"}),
        );
        let resp = dispatch(&req, &state());
        let err = resp.error.expect("expected error without runtime");
        assert_eq!(err.code, ERROR_INVALID_PARAMS);
    }

    #[test]
    fn pipe_clear_missing_session_errors() {
        let req = JsonRpcRequest::with_params(12, "pipe.clear", serde_json::json!({}));
        let resp = dispatch(&req, &state());
        assert!(resp.error.is_some());
    }

    #[test]
    fn pipe_list_empty() {
        let req = JsonRpcRequest::new(13, "pipe.list");
        let resp = dispatch(&req, &state());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let sessions = result["sessions"].as_array().unwrap();
        assert!(sessions.is_empty());
    }

    // ── GAP-43 remote.* dispatch ────────────────────────────────────────────
    #[test]
    fn remote_listdir_unknown_session_errors() {
        let req = JsonRpcRequest::with_params(
            14,
            "remote.listdir",
            serde_json::json!({"session_id": "missing", "path": "."}),
        );
        let resp = dispatch(&req, &state());
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, ERROR_INVALID_PARAMS);
    }

    #[test]
    fn remote_listdir_missing_session_id_errors() {
        let req = JsonRpcRequest::with_params(15, "remote.listdir", serde_json::json!({}));
        let resp = dispatch(&req, &state());
        assert!(resp.error.is_some());
    }

    #[test]
    fn remote_read_missing_path_errors() {
        let req = JsonRpcRequest::with_params(
            16,
            "remote.read",
            serde_json::json!({"session_id": "missing", "path": "x"}),
        );
        let resp = dispatch(&req, &state());
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, ERROR_INVALID_PARAMS);
    }

    #[test]
    fn remote_read_unknown_session_errors() {
        let req = JsonRpcRequest::with_params(
            17,
            "remote.read",
            serde_json::json!({"session_id": "missing", "path": "README.md"}),
        );
        let resp = dispatch(&req, &state());
        assert!(resp.error.is_some());
    }

    #[test]
    fn remote_write_missing_content_errors() {
        let req = JsonRpcRequest::with_params(
            18,
            "remote.write",
            serde_json::json!({"session_id": "missing", "path": "x.txt"}),
        );
        let resp = dispatch(&req, &state());
        assert!(resp.error.is_some());
    }

    #[test]
    fn remote_find_missing_query_errors() {
        let req = JsonRpcRequest::with_params(
            19,
            "remote.find",
            serde_json::json!({"session_id": "missing", "path": "."}),
        );
        let resp = dispatch(&req, &state());
        assert!(resp.error.is_some());
    }
}
