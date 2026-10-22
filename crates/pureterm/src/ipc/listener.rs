//! PureTerm IPC 的异步 Unix socket 监听器。
//!
//! 绑定一个 `tokio::net::UnixListener`,接受连接,每个连接解析一条
//! JSON-RPC 请求,并通过 channel 转发给
//! 主 Iced 事件循环。主循环通过
//! `SocketRouter` 分发方法,并把响应写回客户端。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use log::{error, info, warn};

use super::methods::SocketRouter;
use super::protocol::{JsonRpcRequest, JsonRpcResponse};

/// IPC socket 路径的环境变量名。
const PURETERM_SOCKET_ENV: &str = "PURETERM_SOCKET";

/// 每个 [`IpcRequest`] 携带的延迟回复句柄:一个 oneshot 发送端,
/// 封装在 `Arc<Mutex<Option<_>>>` 中。`oneshot::Sender` 不可 `Clone`,而
/// iced 的 `Message` 需要 `Clone`,因此发送端只能被取用(消费)一次——
/// 即主循环分发回复时,通过 [`take_reply_sender`] 取用。
pub type ReplyTx = std::sync::Arc<
    std::sync::Mutex<Option<tokio::sync::oneshot::Sender<JsonRpcResponse>>>,
>;

/// 从 [`ReplyTx`] 中取走回复发送端,毒化互斥锁时恢复
/// 而不是 panic。浏览器操作解析器与隧道操作运行在 GUI
/// 主循环上;毒化互斥锁绝不能拖垮整个 UI(与
/// `browser::commands::execute` 中 `.lock().ok()` 的约定一致)。先到先得:
/// 迟到的取用者看到 `None` 便不做处理,超时不会重复回复。
pub fn take_reply_sender(
    tx: &ReplyTx,
) -> Option<tokio::sync::oneshot::Sender<JsonRpcResponse>> {
    tx.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// 转发给主循环的待处理 IPC 请求。
#[derive(Debug, Clone)]
pub struct IpcRequest {
    pub id: u64,
    pub method: String,
    pub params: Option<serde_json::Value>,
    pub reply_tx: ReplyTx,
}

/// 启动 IPC 监听器后返回的句柄,携带 socket 路径。
pub struct IpcHandle {
    pub socket_path: PathBuf,
}

/// 启动 IPC 监听器后台任务。成功时返回 socket 路径。
///
/// 监听器与应用程序同生命周期。它通过 `tx`(发送 `IpcRequest`)
/// 与主循环通信,并通过每个请求内嵌的 per-request `oneshot` 通道
/// 接收响应。
pub fn spawn_ipc_listener(
    socket_path: PathBuf,
    router: Arc<SocketRouter>,
    tx: tokio::sync::mpsc::Sender<IpcRequest>,
) -> std::io::Result<IpcHandle> {
    // 清理上次运行遗留的过期 socket。
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }

    // 绑定 socket(此处阻塞无妨——正处于启动阶段)。
    let listener = std::os::unix::net::UnixListener::bind(&socket_path)?;
    listener.set_nonblocking(true)?;

    // 转换为 tokio 监听器。
    let listener = UnixListener::from_std(listener)?;

    // 将 socket 路径注册为环境变量,供 `pureterm msg` 使用。
    std::env::set_var(PURETERM_SOCKET_ENV, &socket_path);
    info!("IPC socket: {}", socket_path.display());

    // 启动接受连接循环。
    tokio::spawn(accept_loop(listener, router, tx));

    Ok(IpcHandle { socket_path })
}

async fn accept_loop(
    listener: UnixListener,
    router: Arc<SocketRouter>,
    tx: tokio::sync::mpsc::Sender<IpcRequest>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let router = router.clone();
                let tx = tx.clone();
                tokio::spawn(handle_connection(stream, router, tx));
            }
            Err(e) => {
                error!("IPC accept error: {e}");
                // 短暂休眠,避免在持续错误时忙循环。
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    router: Arc<SocketRouter>,
    tx: tokio::sync::mpsc::Sender<IpcRequest>,
) {
    // 从客户端读取一行 JSON。
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    match reader.read_line(&mut line).await {
        Ok(0) | Err(_) => return,
        _ => {}
    }

    let line = line.trim();
    if line.is_empty() {
        return;
    }

    // 解析请求。
    let request = match JsonRpcRequest::from_line(line) {
        Ok(r) => r,
        Err(e) => {
            // 无法从畸形请求中确定 id;使用 0。
            let resp = JsonRpcResponse {
                jsonrpc: super::protocol::JSONRPC_VERSION.into(),
                id: 0,
                result: None,
                error: Some(e),
            };
            let _ = write_half.write_all(resp.to_line().as_bytes()).await;
            return;
        }
    };

    // 为响应创建 oneshot 通道。
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<JsonRpcResponse>();

    let ipc_req = IpcRequest {
        id: request.id,
        method: request.method.clone(),
        params: request.params,
        reply_tx: std::sync::Arc::new(std::sync::Mutex::new(Some(reply_tx))),
    };

    // 转发给主循环。
    if tx.send(ipc_req).await.is_err() {
        warn!("IPC: main loop channel closed, dropping request");
        return;
    }

    // 等待主循环处理并回传响应。
    match reply_rx.await {
        Ok(resp) => {
            if let Err(e) = write_half.write_all(resp.to_line().as_bytes()).await {
                warn!("IPC: failed to write response: {e}");
            }
        }
        Err(_) => {
            warn!("IPC: response channel dropped");
        }
    }
}

// ── Socket 路径工具 ──────────────────────────────────────────────

/// IPC socket 文件的目录。
#[cfg(not(target_os = "macos"))]
pub fn socket_dir() -> PathBuf {
    xdg::BaseDirectories::with_prefix("pureterm")
        .get_runtime_directory()
        .map(|p| p.to_owned())
        .ok()
        .and_then(|path| std::fs::create_dir_all(&path).map(|_| path).ok())
        .unwrap_or_else(std::env::temp_dir)
}

/// IPC socket 文件的目录。
#[cfg(target_os = "macos")]
pub fn socket_dir() -> PathBuf {
    std::env::temp_dir()
}

/// 匹配所有可用 socket 的文件前缀。
#[cfg(not(target_os = "macos"))]
pub fn socket_prefix() -> String {
    let display = std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| std::env::var("DISPLAY"))
        .unwrap_or_default();
    format!("PureTerm-{}", display.replace('/', "-"))
}

/// 匹配所有可用 socket 的文件前缀。
#[cfg(target_os = "macos")]
pub fn socket_prefix() -> String {
    String::from("PureTerm")
}

/// 生成当前实例的 socket 路径(含 PID 以保证唯一)。
///
/// Unix socket 的路径长度有硬上限(`sockaddr_un::sun_path`,macOS /
/// Linux 上是 104 字节)。超过时 `bind()` 失败,IPC 监听器起不来 ——
/// 应用照常显示窗口,但所有 CLI / IPC 功能全部失效。
///
/// `TMPDIR` 稍长就会触发(macOS 的 per-user 临时目录本身就接近 50
/// 字节),因此这里在路径放不下时回退到短目录
/// (`/tmp/pureterm-<uid>`),客户端按同一顺序搜索。
pub fn default_socket_path() -> PathBuf {
    let file = format!("{}-{}.sock", socket_prefix(), std::process::id());
    for dir in socket_search_dirs() {
        let candidate = dir.join(&file);
        if pureterm_daemon::discovery::fits_sock_path(&candidate) {
            return candidate;
        }
    }
    pureterm_daemon::discovery::short_socket_dir().join(file)
}

/// 搜索 socket 的候选目录,按优先级排列。
///
/// 与 [`default_socket_path`] 用同一套规则 —— 服务端落在哪个目录,
/// 客户端就去哪些目录找。
pub fn socket_search_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![socket_dir()];
    let short = pureterm_daemon::discovery::short_socket_dir();
    if !dirs.contains(&short) {
        dirs.push(short);
    }
    dirs
}

/// 查找正在运行的 PureTerm 实例的 socket。供 CLI 客户端使用。
pub fn find_socket(socket_path: Option<PathBuf>) -> std::io::Result<crate::local_socket::LocalStream> {
    use std::ffi::OsStr;
    use std::io::{Error as IoError, ErrorKind};

    // 处理显式的 --socket CLI 覆盖参数。
    if let Some(path) = socket_path {
        return crate::local_socket::connect(&path)
            .map_err(|e| IoError::new(e.kind(), format!("invalid socket path {path:?}")));
    }

    // 检查环境变量。
    if let Ok(path) = std::env::var(PURETERM_SOCKET_ENV) {
        let path = PathBuf::from(path);
        if let Ok(stream) = crate::local_socket::connect(&path) {
            return Ok(stream);
        }
    }

    // 搜索 socket 文件。服务端可能在路径过长时回退到短目录,
    // 因此按同样的候选顺序逐个搜索。
    let prefix = socket_prefix();
    for dir in socket_search_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name_matches = path
                .file_name()
                .and_then(OsStr::to_str)
                .filter(|name| name.starts_with(&prefix) && name.ends_with(".sock"))
                .is_some();
            if !name_matches {
                continue;
            }
            match crate::local_socket::connect(&path) {
                Ok(stream) => return Ok(stream),
                Err(e) if e.kind() == ErrorKind::ConnectionRefused => {
                    // 已死实例遗留的孤儿 socket;清理之。
                    let _ = std::fs::remove_file(&path);
                }
                _ => {}
            }
        }
    }

    Err(IoError::new(ErrorKind::NotFound, "no PureTerm socket found"))
}
