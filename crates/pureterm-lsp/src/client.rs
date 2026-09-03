//! rust-analyzer stdio 客户端:initialize、didOpen、publishDiagnostics、hover、completion。

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use pureterm_jsonrpc::JSONRPC_VERSION;
use serde_json::{json, Value};
use thiserror::Error;

use crate::diagnostic::{
    language_id, parse_publish_diagnostics, path_to_uri, DiagnosticStore,
};
use crate::transport::{read_message, write_message};

#[derive(Clone, Debug)]
pub struct LspClientConfig {
    pub command: String,
    /// 传给语言服务器的额外 argv(如 pyright/ts-ls 的 `"--stdio"`)。
    pub args: Vec<String>,
    pub workspace_root: PathBuf,
}

#[derive(Debug, Error)]
pub enum LspError {
    #[error("spawn {0}: {1}")]
    Spawn(String, std::io::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported language for {0}")]
    UnsupportedLanguage(PathBuf),
    #[error("client not running")]
    NotRunning,
    #[error("request timeout")]
    Timeout,
    #[error("lsp error: {0}")]
    Server(String),
}

pub struct LspClient {
    child: Child,
    writer: Arc<Mutex<Option<std::process::ChildStdin>>>,
    store: Arc<Mutex<DiagnosticStore>>,
    pending: Arc<Mutex<std::collections::HashMap<u64, mpsc::Sender<Value>>>>,
    next_id: AtomicU64,
    open_versions: Arc<Mutex<std::collections::HashMap<String, i32>>>,
    /// 由读线程在服务器 stdout 关闭时翻转为 `false`,
    /// 使调用方不再信任缓存的诊断(BUG-3)。
    alive: Arc<AtomicBool>,
}

impl LspClient {
    /// 拉起语言服务器并完成 initialize 握手。
    pub fn spawn(cfg: LspClientConfig) -> Result<Self, LspError> {
        let mut child = Command::new(&cfg.command)
            .args(&cfg.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| LspError::Spawn(cfg.command.clone(), e))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::Io(std::io::Error::other("no stdout")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::Io(std::io::Error::other("no stdin")))?;

        let store = Arc::new(Mutex::new(DiagnosticStore::default()));
        let pending = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let writer = Arc::new(Mutex::new(Some(stdin)));
        let store_reader = Arc::clone(&store);
        let pending_reader = Arc::clone(&pending);
        let alive_reader = Arc::clone(&alive);
        let writer_reader = Arc::clone(&writer);
        thread::spawn(move || {
            reader_loop(stdout, store_reader, pending_reader, alive_reader, writer_reader)
        });

        let client = Self {
            child,
            writer,
            store,
            pending,
            next_id: AtomicU64::new(1),
            open_versions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            alive,
        };

        client.initialize(&cfg.workspace_root)?;
        Ok(client)
    }

    pub fn store(&self) -> Arc<Mutex<DiagnosticStore>> {
        Arc::clone(&self.store)
    }

    /// 读线程是否仍在排空服务器 stdout。
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn summary(&self) -> crate::diagnostic::DiagnosticSummary {
        self.store.lock().summary()
    }

    pub fn context_block(&self) -> String {
        self.store.lock().to_context_block()
    }

    pub fn did_open(&self, path: &Path) -> Result<(), LspError> {
        let lang = language_id(path).ok_or_else(|| LspError::UnsupportedLanguage(path.to_path_buf()))?;
        let uri = path_to_uri(path);
        {
            let mut versions = self.open_versions.lock();
            if versions.contains_key(&uri) {
                // 同一 URI 重复 didOpen 违反 LSP(会被视为文档
                // 重置);首次之后直接跳过。
                return Ok(());
            }
            versions.insert(uri.clone(), 1);
        }
        let text = std::fs::read_to_string(path).map_err(LspError::Io)?;

        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": lang,
                    "version": 1,
                    "text": text,
                }
            }),
        )
    }

    pub fn hover(&self, path: &Path, line: u32, character: u32) -> Result<Option<String>, LspError> {
        let _ = self.did_open(path);
        let uri = path_to_uri(path);
        let result = self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
            Duration::from_millis(800),
        )?;
        Ok(parse_hover_markdown(&result))
    }

    pub fn completion(&self, path: &Path, line: u32, character: u32) -> Result<Vec<String>, LspError> {
        let _ = self.did_open(path);
        let uri = path_to_uri(path);
        let result = self.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
            Duration::from_millis(800),
        )?;
        Ok(parse_completion_labels(&result))
    }

    fn initialize(&self, root: &Path) -> Result<(), LspError> {
        let root_uri = path_to_uri(root);
        // 发送 `initialize` 请求并等待响应,再
        // 宣告 `initialized`(LSP 握手顺序,BUG-2)。读取
        // 响应也能让服务器侧的拒绝 / 能力错误浮出水面,
        // 而不是被静默丢弃。
        let result = self.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "capabilities": {},
                "clientInfo": { "name": "pureterm", "version": env!("CARGO_PKG_VERSION") }
            }),
            Duration::from_secs(3),
        )?;
        // `result` 持有 InitializeResult(服务器能力);留作未来
        // 按特性门控(hover/completion 提供方)之用。MVP 未使用。
        let _ = result;
        self.notify("initialized", json!({}))?;
        Ok(())
    }

    fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().insert(id, tx);
        self.send(json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "method": method,
            "params": params,
        }))?;
        match rx.recv_timeout(timeout) {
            Ok(msg) => {
                if let Some(err) = msg.get("error") {
                    return Err(LspError::Server(err.to_string()));
                }
                Ok(msg.get("result").cloned().unwrap_or(Value::Null))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.pending.lock().remove(&id);
                Err(LspError::Timeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(LspError::NotRunning),
        }
    }

    fn send(&self, value: Value) -> Result<(), LspError> {
        let mut guard = self.writer.lock();
        let stdin = guard.as_mut().ok_or(LspError::NotRunning)?;
        write_message(stdin, &value).map_err(LspError::Io)
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.send(json!({
            "jsonrpc": JSONRPC_VERSION,
            "method": method,
            "params": params,
        }))
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // 杀掉服务器前先请求其优雅关停(BUG-4)。
        // 尽力而为:忽略超时/错误,回退到 SIGKILL。
        let _ = self.request("shutdown", Value::Null, Duration::from_millis(500));
        let _ = self.notify("exit", Value::Null);
        let _ = self.writer.lock().take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn reader_loop(
    stdout: std::process::ChildStdout,
    store: Arc<Mutex<DiagnosticStore>>,
    pending: Arc<Mutex<std::collections::HashMap<u64, mpsc::Sender<Value>>>>,
    alive: Arc<AtomicBool>,
    writer: Arc<Mutex<Option<std::process::ChildStdin>>>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_message(&mut reader) {
            Ok(msg) => {
                // 客户端请求的响应:带 id 且不带 method。
                if msg.get("method").is_none() {
                    if let Some(id) = msg.get("id").and_then(|v| v.as_u64()) {
                        if let Some(tx) = pending.lock().remove(&id) {
                            let _ = tx.send(msg);
                            continue;
                        }
                    }
                } else if let Some(id) = msg.get("id").cloned() {
                    // 服务器→客户端请求(如 workspace/configuration、
                    // client/registerCapability)。本 MVP 未实现任何
                    // server-initiated 特性,按 LSP 规范回
                    // MethodNotFound,防止服务器等待悬挂。
                    let resp = json!({
                        "jsonrpc": JSONRPC_VERSION,
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": "method not supported by pureterm client"
                        }
                    });
                    let mut guard = writer.lock();
                    if let Some(stdin) = guard.as_mut() {
                        let _ = write_message(stdin, &resp);
                    }
                    continue;
                }
                if msg.get("method").and_then(|m| m.as_str())
                    == Some("textDocument/publishDiagnostics")
                {
                    if let Some(params) = msg.get("params") {
                        if let Some((uri, diags)) = parse_publish_diagnostics(params) {
                            store.lock().set_for_uri(&uri, diags);
                        }
                    }
                }
            }
            Err(e) => {
                log::debug!("lsp reader exit: {e}");
                break;
            }
        }
    }
    // 服务器 stdout 关闭/出错:把客户端标记为已死并丢弃过期
    // 诊断,使状态栏 / AI 聊天不再显示它们(BUG-3)。
    alive.store(false, Ordering::SeqCst);
    store.lock().clear();
}

fn parse_hover_markdown(result: &Value) -> Option<String> {
    let contents = result.get("contents")?;
    if let Some(s) = contents.as_str() {
        return Some(s.to_string());
    }
    if let Some(markup) = contents.get("value").and_then(|v| v.as_str()) {
        return Some(markup.to_string());
    }
    None
}

fn parse_completion_labels(result: &Value) -> Vec<String> {
    let items = if result.is_array() {
        result.as_array().cloned().unwrap_or_default()
    } else {
        result
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    items
        .into_iter()
        .filter_map(|item| {
            item.get("label")
                .and_then(|l| l.as_str())
                .map(|s| s.to_string())
        })
        .take(32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_completion_from_array() {
        let v = json!([{"label": "foo"}, {"label": "bar"}]);
        assert_eq!(parse_completion_labels(&v), vec!["foo", "bar"]);
    }

    #[test]
    fn parse_hover_string() {
        let v = json!({ "contents": "hello" });
        assert_eq!(parse_hover_markdown(&v).as_deref(), Some("hello"));
    }
}
