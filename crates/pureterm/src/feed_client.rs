//! GAP-7.4: std-only Feed IPC client used by the `pureterm hook` shim.
//!
//! 当 `evaluate` 产出 `Decision::Ask` 时,shim 用本模块联系运行中的 PureTerm
//! GUI,把审批请求送到右侧 Feed 面板并**阻塞**等待人的决定(最长 `timeout`),
//! 再把决定翻译回 allow/deny。全程 std-only(无 tokio 依赖),复用 GUI IPC 的
//! Unix socket + JSON-RPC 2.0 协议(镜像 `ipc::client::send_rpc`)。
//!
//! 无 GUI / 连接失败 / 超时 → `FeedOutcome::NoGui`/`Timeout`,调用方回退到
//! `permissionDecision:"ask"`(交还 Claude Code 原生 prompt),不阻断工具。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use crate::ipc::listener::find_socket;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

/// 用户在 Feed 面板点的选择。跨 shim↔GUI 的 wire 类型(单一定义点,
/// `feed_panel.rs` 复用)。序列化为小写字符串:`once`/`always`/`all`/`bypass`/`deny`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedChoice {
    /// 仅本次放行(不记忆)。
    Once,
    /// 此会话此工具始终放行(GUI 记忆)。
    Always,
    /// 此 agent(会话)所有工具放行(GUI 记忆)。
    All,
    /// 临时跳过审批(GUI 记忆 + 短期过期)。
    Bypass,
    /// 阻断。
    Deny,
}

impl FeedChoice {
    /// 放行类(Once/Always/All/Bypass)→ `true`;Deny → `false`。
    pub fn is_allow(self) -> bool {
        !matches!(self, FeedChoice::Deny)
    }

    /// wire 字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            FeedChoice::Once => "once",
            FeedChoice::Always => "always",
            FeedChoice::All => "all",
            FeedChoice::Bypass => "bypass",
            FeedChoice::Deny => "deny",
        }
    }

    /// 从 wire 字符串解析(容错:`None` 表示未知值)。
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "once" => FeedChoice::Once,
            "always" => FeedChoice::Always,
            "all" => FeedChoice::All,
            "bypass" => FeedChoice::Bypass,
            "deny" => FeedChoice::Deny,
            _ => return None,
        })
    }
}

/// `feed.request_approval` 的请求参数(shim → GUI)。
#[derive(Debug, serde::Serialize)]
pub struct FeedApprovalRequest<'a> {
    pub session_id: &'a str,
    pub event: &'a str,
    pub tool_name: Option<&'a str>,
    pub tool_input: Option<&'a serde_json::Value>,
    pub prompt: Option<&'a str>,
    pub message: Option<&'a str>,
    pub reason: &'a str,
    pub cwd: &'a str,
}

/// 一次 feed 往返的结果。
#[derive(Debug)]
pub enum FeedOutcome {
    /// GUI 回了人的决定。
    Resolved(FeedChoice),
    /// 没有运行中的 PureTerm GUI(`find_socket` 找不到 socket)。
    NoGui,
    /// GUI 存在但未在 `timeout` 内回复(含 read timeout / EOF)。
    Timeout,
    /// 其它 IO / 协议错误。
    Error(String),
}

/// 联系 GUI 发起审批,阻塞最长 `timeout`。`socket_override` 用于测试/`--socket`。
///
/// 协议:单连接单请求 —— 写一行 JSON-RPC → `shutdown(WRITE)` → 阻塞 `read_line`
/// 等回复(GUI 延迟回复,见 `feed_panel.rs` 的 deferred-reply)。读超时由
/// `set_read_timeout(timeout)` 保证,无需额外线程。
pub fn request_approval(
    req: &FeedApprovalRequest<'_>,
    timeout: Duration,
    socket_override: Option<PathBuf>,
) -> FeedOutcome {
    let mut stream = match find_socket(socket_override) {
        Ok(s) => s,
        Err(_) => return FeedOutcome::NoGui,
    };
    // 读写超时:read 在 GUI 不回时按 `timeout` 触发 TimedOut。
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let params = serde_json::to_value(req).unwrap_or_else(|_| serde_json::json!({}));
    let request = JsonRpcRequest::with_params(id, "feed.request_approval", params);

    let mut line = match serde_json::to_string(&request) {
        Ok(s) => s,
        Err(e) => return FeedOutcome::Error(format!("serialize request: {e}")),
    };
    line.push('\n');

    if let Err(e) = stream.write_all(line.as_bytes()) {
        if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) {
            return FeedOutcome::Timeout;
        }
        return FeedOutcome::Error(format!("write: {e}"));
    }
    if let Err(e) = stream.shutdown(std::net::Shutdown::Write) {
        // shutdown(WRITE) 失败不致命(对端仍可能回),但记录一下。
        log::debug!("feed_client: shutdown write failed: {e}");
    }

    let mut buffer = String::new();
    let mut reader = BufReader::new(&stream);
    match reader.read_line(&mut buffer) {
        Ok(0) => return FeedOutcome::Timeout, // EOF:GUI 接了但没回
        Ok(_) => {}
        Err(e) => {
            if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) {
                return FeedOutcome::Timeout;
            }
            return FeedOutcome::Error(format!("read: {e}"));
        }
    }

    let resp: JsonRpcResponse = match serde_json::from_str(buffer.trim()) {
        Ok(r) => r,
        Err(e) => return FeedOutcome::Error(format!("parse response: {e}")),
    };
    if let Some(err) = resp.error {
        return FeedOutcome::Error(format!("gui error {}: {}", err.code, err.message));
    }
    let result = match resp.result {
        Some(r) => r,
        None => return FeedOutcome::Error("gui returned no result".into()),
    };
    let choice_str = match result.get("choice").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return FeedOutcome::Error(format!("result missing 'choice': {result}")),
    };
    match FeedChoice::parse(choice_str) {
        Some(c) => FeedOutcome::Resolved(c),
        None => FeedOutcome::Error(format!("unknown choice: {choice_str}")),
    }
}

/// GAP-7.4: fire-and-forget 通知 GUI 清掉某 agent 会话的 Feed 记忆(SessionEnd)。
///
/// 发送 `feed.clear_session { session_id }` 后立即 `shutdown(WRITE)` 不读回复
/// —— GUI 的 `Message::Ipc` 已在 reply 之前执行了 `FeedClearSession` 副作用,故
/// 即便客户端不读、连接关闭,记忆也已被清。任何错误(GUI 未运行等)静默忽略。
pub fn notify_session_end(session_id: &str) {
    use std::io::Write;
    let mut stream = match find_socket(None) {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    static NEXT_NOTIFY_ID: AtomicU64 = AtomicU64::new(1);
    let id = NEXT_NOTIFY_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let request = JsonRpcRequest::with_params(
        id,
        "feed.clear_session",
        serde_json::json!({ "session_id": session_id }),
    );
    let Ok(mut line) = serde_json::to_string(&request) else { return };
    line.push('\n');
    let _ = stream.write_all(line.as_bytes());
    let _ = stream.shutdown(std::net::Shutdown::Write);
    // 故意不读回复(fire-and-forget)。
}

#[cfg(test)]
mod tests {
    #![allow(clippy::redundant_closure_call)]

    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::OnceLock;
    use std::thread;

    fn bash_cmd() -> serde_json::Value {
        serde_json::json!({ "command": "rm -rf /tmp/x" })
    }

    fn sample_req<'a>(reason: &'a str, tool_input: &'a serde_json::Value) -> FeedApprovalRequest<'a> {
        FeedApprovalRequest {
            session_id: "s1",
            event: "PreToolUse",
            tool_name: Some("Bash"),
            tool_input: Some(tool_input),
            prompt: None,
            message: None,
            reason,
            cwd: "/tmp",
        }
    }

    /// 取一个唯一的临时 socket 路径(不创建文件,只生成路径名)。
    fn unique_socket_path(label: &str) -> PathBuf {
        static COUNTER: OnceLock<AtomicU64> = OnceLock::new();
        let counter = COUNTER.get_or_init(|| AtomicU64::new(0));
        let n = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("pureterm_feed_test_{label}_{n}_{pid}.sock", pid = std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn feed_choice_serde_roundtrip() {
        for c in [
            FeedChoice::Once,
            FeedChoice::Always,
            FeedChoice::All,
            FeedChoice::Bypass,
            FeedChoice::Deny,
        ] {
            let s = serde_json::to_string(&c).unwrap();
            assert_eq!(s, format!("\"{}\"", c.as_str()));
            let back: FeedChoice = serde_json::from_str(&s).unwrap();
            assert_eq!(back, c);
        }
        assert!(FeedChoice::parse("bogus").is_none());
    }

    #[test]
    fn feed_choice_is_allow() {
        assert!(FeedChoice::Once.is_allow());
        assert!(FeedChoice::Bypass.is_allow());
        assert!(!FeedChoice::Deny.is_allow());
    }

    /// 起一个假 GUI listener:bind 后经 `ready` 通道通知已就绪,再 accept 一条
    /// 连接,读请求并按 `reply_choice` 回复。返回 (join handle, ready 接收端)。
    ///
    /// ready 信号保证 client 在 server 真正 listen 之后再 connect —— 消除
    /// 「client 先 connect、server 还没 bind」的竞态;该竞态会让 client 立刻
    /// 拿到 NoGui 返回,而 server 永远阻塞在 accept(),导致测试 `h.join()` 死锁。
    /// bind 失败时 ready_tx drop → `recv()` 快速失败(测试失败而非挂死)。
    fn spawn_fake_gui(
        path: PathBuf,
        reply_choice: Option<&'static str>,
    ) -> (thread::JoinHandle<()>, std::sync::mpsc::Receiver<()>) {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let h = thread::spawn(move || {
            let listener = match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("fake_gui bind failed: {e}");
                    return;
                }
            };
            // 已 bind + listen → 通知 client 可以 connect 了。
            let _ = ready_tx.send(());
            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(_) => return,
            };
            // 读请求(到 EOF,client shutdown(WRITE) 提供)。
            let mut buf = String::new();
            use std::io::Read;
            let _ = stream.read_to_string(&mut buf);
            if let Some(choice) = reply_choice {
                let id: u64 = buf
                    .split("\"id\":")
                    .nth(1)
                    .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                let resp = format!(
                    "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"choice\":\"{choice}\"}}}}\n"
                );
                let _ = stream.write_all(resp.as_bytes());
            }
            // reply_choice = None → 不回复(用于超时测试)
        });
        (h, ready_rx)
    }

    #[test]
    fn request_approval_resolved_once() {
        let path = unique_socket_path("once");
        let (h, ready) = spawn_fake_gui(path.clone(), Some("once"));
        ready.recv_timeout(Duration::from_secs(2)).expect("fake_gui bind");
        let out = request_approval(&sample_req("r", &bash_cmd()), Duration::from_secs(5), Some(path));
        h.join().unwrap();
        assert!(matches!(out, FeedOutcome::Resolved(FeedChoice::Once)), "got {out:?}");
    }

    #[test]
    fn request_approval_resolved_deny() {
        let path = unique_socket_path("deny");
        let (h, ready) = spawn_fake_gui(path.clone(), Some("deny"));
        ready.recv_timeout(Duration::from_secs(2)).expect("fake_gui bind");
        let out = request_approval(&sample_req("r", &bash_cmd()), Duration::from_secs(5), Some(path));
        h.join().unwrap();
        assert!(matches!(out, FeedOutcome::Resolved(FeedChoice::Deny)), "got {out:?}");
    }

    #[test]
    fn request_approval_no_gui_when_socket_missing() {
        let path = unique_socket_path("nogui"); // 不 bind → connect 失败
        let out = request_approval(&sample_req("r", &bash_cmd()), Duration::from_secs(2), Some(path));
        assert!(matches!(out, FeedOutcome::NoGui), "got {out:?}");
    }

    #[test]
    fn request_approval_timeout_when_gui_silent() {
        let path = unique_socket_path("silent");
        let (h, ready) = spawn_fake_gui(path.clone(), None); // 接受但不回
        ready.recv_timeout(Duration::from_secs(2)).expect("fake_gui bind");
        let out = request_approval(&sample_req("r", &bash_cmd()), Duration::from_millis(500), Some(path));
        h.join().unwrap();
        assert!(matches!(out, FeedOutcome::Timeout), "got {out:?}");
    }
}
