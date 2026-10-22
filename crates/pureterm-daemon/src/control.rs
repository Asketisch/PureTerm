//! `puretermd` 的 JSON-RPC 2.0 控制面。
//!
//! 一次性的 Unix socket 服务器(每连接一个 JSON-RPC 请求/响应,
//! 镜像 GUI IPC 监听器),暴露 `daemon.*` 与
//! `session.*` 方法。数据面(流式 PTY 字节)是 `session.attach`
//! 返回的独立每会话分帧 socket。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use pureterm_jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, ERROR_INVALID_PARAMS};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use pureterm_ssh::SshForwards;

use crate::session::SessionManager;

/// 共享的控制面状态。
#[derive(Clone)]
pub struct ControlState {
    pub manager: SessionManager,
    pub started: Instant,
    /// 启动时捕获的 `puretermd` 可执行文件 mtime(Unix 纪元秒)。
    /// 让 GUI 能检测陈旧守护进程(重新打包后仍在跑旧二进制),
    /// 并重启它使新代码真正生效。
    /// 读不到可执行路径/mtime 时为 0。
    pub exe_mtime: u64,
    /// GAP-5.2:多线程 runtime 的句柄,使(同步的)`tunnel.*`
    /// 方法能派发异步 russh 连接/拷贝任务。单元测试中为 `None`
    /// (未启动 runtime)——此时 `tunnel.start` 报 "no runtime"。
    pub runtime: Option<tokio::runtime::Handle>,
    /// GAP-5.2:存活的 russh SOCKS5 隧道。
    pub tunnels: crate::tunnel::TunnelManager,
    /// GAP-6.6:wait-for 通道/锁存储(tmux `wait-for`)。等待者在此
    /// 驻留,直到 signaler 唤醒(或守护进程关闭)。
    pub wait_channels: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Vec<tokio::sync::oneshot::Sender<()>>>>>,
    /// 冷启动闸门:`deferred_init` 完成绑定后的非关键初始化
    /// (known_hosts、隧道恢复、CPU 采样器)之前为 `false`。
    /// 在这约几十毫秒的窗口内 `tunnel.start` 返回可重试错误,
    /// 而不是启动一条 TOFU 主机密钥存储尚未加载的隧道。
    pub init_done: Arc<std::sync::atomic::AtomicBool>,
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
            init_done: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// 生产构造函数:附上守护进程的 runtime 句柄,使隧道
    /// 任务可被派发。(`new` 保留 runtime = None,供单元测试。)
    pub fn with_runtime(manager: SessionManager, runtime: tokio::runtime::Handle) -> Self {
        let mut s = Self::new(manager);
        s.runtime = Some(runtime);
        s
    }
}

/// GAP-5.2:把存活隧道配置快照到 `tunnels.json`(守护进程
/// 重启后仅恢复配置的持久化)。尽力而为(save_tunnels 失败
/// 仅记录日志)——在每次 `tunnel.start`/`tunnel.stop` 变更后调用。
fn persist_tunnels(state: &ControlState) {
    let cfg = crate::persist::TunnelsFile {
        version: 1,
        tunnels: state.tunnels.snapshot_configs(),
    };
    crate::persist::save_tunnels(&crate::discovery::tunnels_file(), &cfg);
}

/// 非关键的守护进程初始化,在控制 socket 绑定之后运行(见
/// `serve`),使冷启动的 GUI 无需等待这些设置即可抵达
/// `daemon.ping`。这些步骤此前内联在 `runtime::run` 中、先于
/// `UnixListener::bind` 执行,使 socket 被 known_hosts/隧道恢复/CPU
/// 工作门控,推高了冷启动延迟。
fn deferred_init(state: Arc<ControlState>) {
    // GAP-5.2:打开 TOFU known_hosts 存储并交给隧道
    // 管理器。打开失败(罕见:数据目录不可读)降级为遗留的
    // 接受一切主机密钥行为,而非让守护进程崩溃。
    match crate::known_hosts::KnownHosts::open(crate::discovery::known_hosts_file()) {
        Ok(kh) => state.tunnels.set_known_hosts(Arc::new(kh)),
        Err(e) => log::warn!("known_hosts: open failed, TOFU disabled: {e}"),
    }

    // GAP-5.2:仅恢复配置——重载隧道配置使其在守护进程
    // 重启后存活,注册为 Stopped(无监听器/任务)。用户
    // 经 tunnel.start 显式重启。机密从未持久化,
    // 因此密码认证隧道重启后重新进入 NeedAuth。
    if let Some(file) = crate::persist::load_tunnels(&crate::discovery::tunnels_file()) {
        let n = file.tunnels.len();
        for cfg in file.tunnels {
            state.tunnels.register_stopped(cfg);
        }
        if n > 0 {
            log::info!("restored {n} tunnel config(s) (config-only; use tunnel.start to connect)");
        }
    }

    // GAP-7.3:周期 CPU 采样 tick 驱动每个会话的 `cpu_idle_secs`
    // ——与泵记录的 PTY 输出空闲并列的休眠双重空闲条件。
    // 与控制监听器并发运行。
    if let Some(rt) = state.runtime.as_ref().cloned() {
        let tick_state = state.clone();
        rt.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                tick_state.manager.sample_cpu_pass();
            }
        });
    }

    // 标记初始化完成:`tunnel.start`(被此标志 TOFU 门控)现在可以继续。
    state
        .init_done
        .store(true, std::sync::atomic::Ordering::SeqCst);
}

/// 绑定控制 socket 并服务,直到 runtime 被取消。
pub async fn serve(socket_path: PathBuf, state: Arc<ControlState>) -> std::io::Result<()> {
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    let listener = std::os::unix::net::UnixListener::bind(&socket_path)?;
    listener.set_nonblocking(true)?;
    crate::discovery::restrict_socket(&socket_path); // GAP-5.1 M5: owner-only
    let listener = UnixListener::from_std(listener)?;
    log::info!("puretermd control socket: {}", socket_path.display());

    // 冷启动响应性:非关键初始化(known_hosts、隧道
    // 恢复、CPU 采样器)在 socket 绑定之后运行,使 GUI 的
    // spawn 轮询尽早发现守护进程。这些绝不阻塞服务——`register_stopped`
    // 只插入 Stopped 条目,且 `tunnel.start` 被 `init_done` 门控。
    let init_state = state.clone();
    tokio::spawn(async move {
        deferred_init(init_state);
    });

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

    // GAP-6.6:`wait_for.*` 是异步的——`wait`/`lock` 使调用方驻留。路由到
    // 这里(先于同步 `dispatch`),因为阻塞需要 await。
    // 每条连接是独立的 tokio 任务,因此驻留的等待者只阻塞
    // 自己,不影响 accept 循环或其它调用方。
    let is_shutdown = request.method == "daemon.shutdown";
    let response = if request.method.starts_with("wait_for.") {
        crate::wait_for::handle(&request, &state).await
    } else {
        dispatch(&request, &state)
    };
    let _ = write_half.write_all(response.to_line().as_bytes()).await;
    if is_shutdown {
        shutdown_and_exit(&state).await;
    }
}

/// `daemon.shutdown` 的实际执行:杀掉全部会话、清掉控制 socket、退出进程。
///
/// 应答**先**写出去(调用方要看到 `{"ok": true}`),所以这里先等一小会儿
/// 让 socket 把数据刷出去,再收工。
async fn shutdown_and_exit(state: &ControlState) {
    // 给应答一点时间刷出去。
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    for info in state.manager.list() {
        state.manager.kill(&info.id);
    }
    // 清理控制 socket —— 下次启动会重建它。
    let _ = std::fs::remove_file(crate::discovery::socket_path());
    std::process::exit(0);
}

/// 把 JSON-RPC 请求路由到其处理器。
pub fn dispatch(request: &JsonRpcRequest, state: &ControlState) -> JsonRpcResponse {
    let id = request.id;
    let result: Result<serde_json::Value, JsonRpcError> = match request.method.as_str() {
        "daemon.ping" => Ok(serde_json::json!({"ok": true})),
        // 优雅关闭:回完应答后杀掉全部会话并退出进程。
        //
        // 为什么要它:守护进程**故意**在 GUI 退出后继续存活(会话要扛过
        // GUI 重启),但它**没有**任何自停手段 —— 用户既没法回收它,
        // 自动化测试也会每个用例泄漏一个(几百个 `puretermd` 足以把
        // 机器拖垮:E2E 套件耗时从 85s 涨到 345s,大量用例超时)。
        "daemon.shutdown" => {
            let sessions = state.manager.list().len();
            Ok(serde_json::json!({"ok": true, "sessions": sessions}))
        },
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
                // GAP-5.1 M3:守护进程持有的 SSH PTY。`password` 由 GUI
                // 解锁并经本地控制 socket 传入;它驱动
                // SSH_ASKPASS。None => 密钥/agent 认证或交互式提示。
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
                // GAP-5.4:可选的 ssh-agent 转发 + 显式 agent socket。
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
                // GAP-7.5:可选的显式 argv(冷恢复 agent)+ 影响
                // 休眠资格(GAP-7.3)的 agent_kind 标签。
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
        // ── agent.* 方法(GAP-7.3 休眠控制面) ─────────────────────────────────────────
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
        // ── tunnel.* 方法(GAP-5.2 russh SOCKS5 隧道控制面) ─────────────────────────────────
        "tunnel.start" => {
            // russh 连接/认证是异步的,但 dispatch 是同步的:同步绑定
            // 监听器 + 注册隧道,在 runtime 上派发异步
            // 连接,立即返回 id。状态经 tunnel.list 轮询
            //(connecting → connected | need_auth | failed)。状态经 tunnel.list 轮询。
            let Some(rt) = state.runtime.as_ref().cloned() else {
                return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "daemon runtime unavailable");
            };
            // 冷启动 TOFU 闸门:known_hosts 由 `deferred_init` 加载
            //(为快速启动推迟到 socket 绑定之后)。在此之前启动隧道
            // 会跳过 TOFU 主机密钥校验。可重试——冷拉起后
            // 初始化窗口仅约几十毫秒。
            if !state.init_done.load(std::sync::atomic::Ordering::SeqCst) {
                return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "daemon initializing, retry");
            }
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
            // GAP-5.4:隧道会话上可选的 ssh-agent 转发。
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
        // ── pipe.* 方法(GAP-6.6 pipe-pane——把会话 PTY 接到命令) ─────────────────────────────
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
        // ── remote.* 方法(GAP-43 文件浏览 MVP) ────────────────────────────────────────────
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
        // `method_err` 已构建带 id 的错误;直接复用。
        Err(e) => JsonRpcResponse {
            jsonrpc: pureterm_jsonrpc::JSONRPC_VERSION.into(),
            id,
            result: None,
            error: Some(JsonRpcError { code: e.code, message: e.message, data: None }),
        },
    }
}

/// 构建一个错误(`id` 字段在此未用;dispatch 在出口处设置它)。
fn method_err(_id: u64, code: i64, message: String) -> JsonRpcError {
    JsonRpcError { code, message, data: None }
}

fn uparam(request: &JsonRpcRequest, key: &str) -> Option<u16> {
    request.params.as_ref()?.get(key)?.as_u64().map(|n| n as u16)
}

fn sparam(request: &JsonRpcRequest, key: &str) -> Option<String> {
    request.params.as_ref()?.get(key)?.as_str().map(|s| s.to_string())
}

/// GAP-5.2:解析 JSON 布尔参数(如 `remote_dns`)。
fn bparam(request: &JsonRpcRequest, key: &str) -> Option<bool> {
    request.params.as_ref()?.get(key)?.as_bool()
}

/// GAP-7.5:解析 JSON 字符串数组参数(如冷恢复 agent 的 `argv`,
/// 经 `claude --resume <id>`)。非字符串元素被丢弃。
fn aparam(request: &JsonRpcRequest, key: &str) -> Option<Vec<String>> {
    request
        .params
        .as_ref()?
        .get(key)?
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
}

/// GAP-40:从 `session.create` 解析 `-L`/`-R`/`-X`/`-Y` 旋钮。
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

    /// `daemon.shutdown` 必须回一个成功应答(调用方据此确认「已收到,
    /// 马上退出」)。真正的退出在 `handle_connection` 里做 —— 这里只
    /// 覆盖分发的部分(否则单测会把自己杀掉)。
    #[test]
    fn shutdown_acks_with_session_count() {
        let req = JsonRpcRequest::new(11, "daemon.shutdown");
        let resp = dispatch(&req, &state());
        assert!(resp.error.is_none(), "shutdown 应被接受: {:?}", resp.error);
        let result = resp.result.unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["sessions"], 0);
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
        // 无可用 shell 的环境里 create 可能失败;
        // 仅在成功时断言。
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

    // ── GAP-5.2 tunnel.* 分派测试 ──────────────────────────────────────────────────
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
        // 单元测试的 ControlState 其 runtime = None,因此 start 必须拒绝,
        // 而不是启动一条它无法驱动的隧道。
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
        // host 缺失先于 runtime 守卫被检查。
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

    // ── GAP-6.6 pipe.* 分派测试 ───────────────────────────────────────────────────
    #[test]
    fn pipe_set_without_runtime_errors() {
        // 单元测试的 ControlState 其 runtime = None,因此 pipe.set 必须拒绝
        //(它需要 runtime 来派发子任务)。
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

    // ── GAP-43 remote.* 分派测试 ────────────────────────────────────────────────────
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
