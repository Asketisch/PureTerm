//! Async Unix socket listener for PureTerm IPC.
//!
//! Binds a `tokio::net::UnixListener`, accepts connections, parses one
//! JSON-RPC request per connection, and forwards it via a channel to the
//! main Iced event loop.  The main loop dispatches the method via
//! `SocketRouter` and writes the response back to the client.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use log::{error, info, warn};

use super::methods::SocketRouter;
use super::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Environment variable name for the IPC socket path.
const PURETERM_SOCKET_ENV: &str = "PURETERM_SOCKET";

/// The deferred-reply handle carried by every [`IpcRequest`]: a oneshot sender
/// behind an `Arc<Mutex<Option<_>>>`. `oneshot::Sender` is not `Clone`, but
/// iced's `Message` needs `Clone`, so the sender is taken (consumed) exactly
/// once — via [`take_reply_sender`] — when the main loop dispatches the reply.
pub type ReplyTx = std::sync::Arc<
    std::sync::Mutex<Option<tokio::sync::oneshot::Sender<JsonRpcResponse>>>,
>;

/// Take the reply sender from a [`ReplyTx`], recovering from a poisoned mutex
/// instead of panicking. The browser op resolver and tunnel ops run on the GUI
/// main loop; a poisoned mutex must not take the whole UI down (mirrors the
/// `.lock().ok()` discipline in `browser::commands::execute`). First-wins: a
/// late taker finds `None` and no-ops, so a timeout can't double-reply.
pub fn take_reply_sender(
    tx: &ReplyTx,
) -> Option<tokio::sync::oneshot::Sender<JsonRpcResponse>> {
    tx.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// A pending IPC request forwarded to the main loop.
#[derive(Debug, Clone)]
pub struct IpcRequest {
    pub id: u64,
    pub method: String,
    pub params: Option<serde_json::Value>,
    pub reply_tx: ReplyTx,
}

/// Handle returned after spawning the IPC listener. Carries the socket path.
pub struct IpcHandle {
    pub socket_path: PathBuf,
}

/// Spawn the IPC listener background task. Returns the socket path on success.
///
/// The listener runs for the lifetime of the application. It communicates
/// with the main loop through `tx` (sends `IpcRequest`s) and receives
/// responses through per-request `oneshot` channels embedded in each request.
pub fn spawn_ipc_listener(
    socket_path: PathBuf,
    router: Arc<SocketRouter>,
    tx: tokio::sync::mpsc::Sender<IpcRequest>,
) -> std::io::Result<IpcHandle> {
    // Clean up any stale socket from a previous run.
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }

    // Bind the socket (blocking is fine here — we're in startup).
    let listener = std::os::unix::net::UnixListener::bind(&socket_path)?;
    listener.set_nonblocking(true)?;

    // Convert to tokio listener.
    let listener = UnixListener::from_std(listener)?;

    // Register socket path as environment variable for `pureterm msg`.
    std::env::set_var(PURETERM_SOCKET_ENV, &socket_path);
    info!("IPC socket: {}", socket_path.display());

    // Spawn accept loop.
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
                // Brief sleep to avoid busy-loop on persistent errors.
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
    // Read one JSON line from the client.
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

    // Parse the request.
    let request = match JsonRpcRequest::from_line(line) {
        Ok(r) => r,
        Err(e) => {
            // Can't determine id from malformed request; use 0.
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

    // Create oneshot channel for the response.
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<JsonRpcResponse>();

    let ipc_req = IpcRequest {
        id: request.id,
        method: request.method.clone(),
        params: request.params,
        reply_tx: std::sync::Arc::new(std::sync::Mutex::new(Some(reply_tx))),
    };

    // Forward to main loop.
    if tx.send(ipc_req).await.is_err() {
        warn!("IPC: main loop channel closed, dropping request");
        return;
    }

    // Wait for the main loop to process and send back the response.
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

// ── Socket path utilities ──────────────────────────────────────────────

/// Directory for the IPC socket file.
#[cfg(not(target_os = "macos"))]
pub fn socket_dir() -> PathBuf {
    xdg::BaseDirectories::with_prefix("pureterm")
        .get_runtime_directory()
        .map(|p| p.to_owned())
        .ok()
        .and_then(|path| std::fs::create_dir_all(&path).map(|_| path).ok())
        .unwrap_or_else(std::env::temp_dir)
}

/// Directory for the IPC socket file.
#[cfg(target_os = "macos")]
pub fn socket_dir() -> PathBuf {
    std::env::temp_dir()
}

/// File prefix matching all available sockets.
#[cfg(not(target_os = "macos"))]
pub fn socket_prefix() -> String {
    let display = std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| std::env::var("DISPLAY"))
        .unwrap_or_default();
    format!("PureTerm-{}", display.replace('/', "-"))
}

/// File prefix matching all available sockets.
#[cfg(target_os = "macos")]
pub fn socket_prefix() -> String {
    String::from("PureTerm")
}

/// Generate a socket path for this instance (includes PID for uniqueness).
pub fn default_socket_path() -> PathBuf {
    let mut path = socket_dir();
    path.push(format!("{}-{}.sock", socket_prefix(), std::process::id()));
    path
}

/// Find a running PureTerm instance's socket. Used by the CLI client.
pub fn find_socket(socket_path: Option<PathBuf>) -> std::io::Result<std::os::unix::net::UnixStream> {
    use std::ffi::OsStr;
    use std::io::{Error as IoError, ErrorKind};

    // Handle explicit --socket CLI override.
    if let Some(path) = socket_path {
        return std::os::unix::net::UnixStream::connect(&path)
            .map_err(|e| IoError::new(e.kind(), format!("invalid socket path {path:?}")));
    }

    // Check environment variable.
    if let Ok(path) = std::env::var(PURETERM_SOCKET_ENV) {
        let path = PathBuf::from(path);
        if let Ok(stream) = std::os::unix::net::UnixStream::connect(&path) {
            return Ok(stream);
        }
    }

    // Search for socket files.
    let prefix = socket_prefix();
    for entry in std::fs::read_dir(socket_dir())?.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name_matches = path
            .file_name()
            .and_then(OsStr::to_str)
            .filter(|name| name.starts_with(&prefix) && name.ends_with(".sock"))
            .is_some();
        if !name_matches {
            continue;
        }
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(stream) => return Ok(stream),
            Err(e) if e.kind() == ErrorKind::ConnectionRefused => {
                // Orphan socket from a dead instance; clean it up.
                let _ = std::fs::remove_file(&path);
            }
            _ => {}
        }
    }

    Err(IoError::new(ErrorKind::NotFound, "no PureTerm socket found"))
}
