//! rust-analyzer stdio client: initialize, didOpen, publishDiagnostics, hover, completion.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
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
}

impl LspClient {
    /// Spawn language server and complete the initialize handshake.
    pub fn spawn(cfg: LspClientConfig) -> Result<Self, LspError> {
        let mut child = Command::new(&cfg.command)
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
        let store_reader = Arc::clone(&store);
        let pending_reader = Arc::clone(&pending);
        thread::spawn(move || reader_loop(stdout, store_reader, pending_reader));

        let writer = Arc::new(Mutex::new(Some(stdin)));
        let client = Self {
            child,
            writer,
            store,
            pending,
            next_id: AtomicU64::new(1),
            open_versions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        client.initialize(&cfg.workspace_root)?;
        Ok(client)
    }

    pub fn store(&self) -> Arc<Mutex<DiagnosticStore>> {
        Arc::clone(&self.store)
    }

    pub fn summary(&self) -> crate::diagnostic::DiagnosticSummary {
        self.store.lock().summary()
    }

    pub fn context_block(&self) -> String {
        self.store.lock().to_context_block()
    }

    pub fn did_open(&self, path: &Path) -> Result<(), LspError> {
        let lang = language_id(path).ok_or_else(|| LspError::UnsupportedLanguage(path.to_path_buf()))?;
        let text = std::fs::read_to_string(path).map_err(LspError::Io)?;
        let uri = path_to_uri(path);
        let mut versions = self.open_versions.lock();
        let version = versions.get(&uri).copied().unwrap_or(0) + 1;
        versions.insert(uri.clone(), version);
        drop(versions);

        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": lang,
                    "version": version,
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
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.send(json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "rootUri": root_uri,
                "capabilities": {},
                "clientInfo": { "name": "pureterm", "version": env!("CARGO_PKG_VERSION") }
            }
        }))?;
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
        let _ = self.writer.lock().take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn reader_loop(
    stdout: std::process::ChildStdout,
    store: Arc<Mutex<DiagnosticStore>>,
    pending: Arc<Mutex<std::collections::HashMap<u64, mpsc::Sender<Value>>>>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_message(&mut reader) {
            Ok(msg) => {
                if let Some(id) = msg.get("id").and_then(|v| v.as_u64()) {
                    if let Some(tx) = pending.lock().remove(&id) {
                        let _ = tx.send(msg);
                        continue;
                    }
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
