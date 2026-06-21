//! GAP-7.4: 右侧 Feed 审批面板 —— GUI 侧状态机。
//!
//! 接收来自 `pureterm hook` shim 的 `feed.request_approval` IPC 请求(经
//! `Message::Ipc` 的 deferred-reply 分支),把请求推入 pending 队列并在右侧
//! pinned overlay 面板展示。用户点 Once/Always/All/Bypass/Deny 后,把决定
//! 经原 IPC 连接的 oneshot 回复给 shim(解锁其阻塞的 `read_line`)。
//!
//! 作用域记忆(`Always`/`All`/`Bypass`)在此维护 —— shim 每次工具调用都是新进程、
//! 无状态,故记忆必须在 GUI 侧。120s 软超时由 `FeedTick` 扫描 → 自动 Deny。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::feed_client::FeedChoice;
use crate::ipc::protocol::JsonRpcResponse;

/// 120s 软超时:超时默认 Deny(D5)。
pub const FEED_SOFT_TIMEOUT: Duration = Duration::from_secs(120);
/// Bypass(临时)窗口:+5min 后过期(D3)。
pub const BYPASS_WINDOW: Duration = Duration::from_secs(5 * 60);

/// deferred-reply 句柄类型 —— 与 `IpcRequest.reply_tx` 完全一致(`Arc<Mutex<
/// Option<oneshot::Sender>>>`,因 iced `Message: Clone` 而 Arc 包裹)。
pub type ReplyHandle = Arc<Mutex<Option<tokio::sync::oneshot::Sender<JsonRpcResponse>>>>;

/// 一次审批请求(GUI 侧拥有的形态)。由 `feed.request_approval` 的 IPC params
/// 解析而来;`ipc_id` 即 `IpcRequest.id`,用作 deferred reply 的键。
#[derive(Debug, Clone)]
pub struct FeedRequestPayload {
    pub ipc_id: u64,
    pub session_id: String,
    pub event: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub prompt: Option<String>,
    pub message: Option<String>,
    pub reason: String,
    pub cwd: Option<String>,
    pub created_at: Instant,
}

impl FeedRequestPayload {
    /// 匹配主体(面板展示 + 审计):Bash command → file_path → 序列化 tool_input
    /// → prompt → message。与 shim 侧 `deny_subject` 优先级一致。
    pub fn subject(&self) -> Option<String> {
        if let Some(ti) = &self.tool_input {
            if let Some(cmd) = ti.get("command").and_then(|v| v.as_str()) {
                return Some(cmd.to_string());
            }
            if let Some(p) = ti.get("file_path").and_then(|v| v.as_str()) {
                return Some(p.to_string());
            }
            return Some(ti.to_string());
        }
        self.prompt.clone().or_else(|| self.message.clone())
    }
}

/// pending 请求 + 其 deferred-reply 句柄。
pub struct PendingRequest {
    pub req: FeedRequestPayload,
    pub reply: ReplyHandle,
}

/// 决定来源(审计用)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveSource {
    /// 用户在面板点了按钮。
    User,
    /// 120s 软超时自动 Deny。
    Timeout,
    /// 记忆命中,直接回复(没弹面板)。
    Memory,
}

/// Feed 面板状态。始终存在于 app(记忆常驻);面板可见性 = `focused.is_some()`。
pub struct FeedPanelState {
    /// pending:以 ipc_id 为键。
    pub pending: HashMap<u64, PendingRequest>,
    /// 当前面板聚焦展示的请求 ipc_id(最近一条)。
    pub focused: Option<u64>,
    /// 作用域记忆(会话级,进程退出即清)。
    always_remember: HashMap<(String, String), FeedChoice>, // (session_id, tool_name)
    all_remember: HashMap<String, FeedChoice>, // session_id
    bypass_until: HashMap<String, Instant>, // session_id → 过期时刻
    /// 最近已决(展示用,有界 100)。
    pub history: Vec<(u64, FeedChoice, ResolveSource)>,
}

impl Default for FeedPanelState {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            focused: None,
            always_remember: HashMap::new(),
            all_remember: HashMap::new(),
            bypass_until: HashMap::new(),
            history: Vec::new(),
        }
    }
}

impl FeedPanelState {
    /// 记忆查询:bypass(未过期) > all > always。返回 `Some` 即命中(立即回复、不弹面板)。
    pub fn consult_memory(&self, session_id: &str, tool_name: &str) -> Option<FeedChoice> {
        let now = Instant::now();
        if let Some(&until) = self.bypass_until.get(session_id) {
            if until > now {
                return Some(FeedChoice::Bypass);
            }
        }
        if let Some(&c) = self.all_remember.get(session_id) {
            return Some(c);
        }
        if let Some(&c) = self.always_remember.get(&(session_id.to_string(), tool_name.to_string())) {
            return Some(c);
        }
        None
    }

    /// 按作用域记录记忆(仅 user 决定调用)。
    pub fn remember(&mut self, session_id: &str, tool_name: &str, choice: FeedChoice) {
        match choice {
            FeedChoice::Always => {
                self.always_remember
                    .insert((session_id.to_string(), tool_name.to_string()), FeedChoice::Always);
            }
            FeedChoice::All => {
                self.all_remember.insert(session_id.to_string(), FeedChoice::All);
            }
            FeedChoice::Bypass => {
                self.bypass_until.insert(session_id.to_string(), Instant::now() + BYPASS_WINDOW);
            }
            // Once / Deny 不记忆。
            _ => {}
        }
    }

    /// 会话结束:清该会话所有记忆(SessionEnd hook)。
    pub fn clear_session(&mut self, session_id: &str) {
        self.always_remember.retain(|(sid, _), _| sid != session_id);
        self.all_remember.remove(session_id);
        self.bypass_until.remove(session_id);
    }

    /// 解析 `feed.request_approval` 的 params → `FeedRequestPayload`。
    /// 必填 `session_id` + `event`;其余可选。
    pub fn parse_payload(ipc_id: u64, params: &Value) -> Option<FeedRequestPayload> {
        let s = params.as_object()?;
        let session_id = s.get("session_id").and_then(|v| v.as_str())?.to_string();
        let event = s.get("event").and_then(|v| v.as_str())?.to_string();
        Some(FeedRequestPayload {
            ipc_id,
            session_id,
            event,
            tool_name: s.get("tool_name").and_then(|v| v.as_str()).map(String::from),
            tool_input: s.get("tool_input").cloned(),
            prompt: s.get("prompt").and_then(|v| v.as_str()).map(String::from),
            message: s.get("message").and_then(|v| v.as_str()).map(String::from),
            reason: s.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            cwd: s.get("cwd").and_then(|v| v.as_str()).map(String::from),
            created_at: Instant::now(),
        })
    }

    /// 解析 `feed.clear_session` 的 params → session_id。
    pub fn parse_session_id(params: &Value) -> Option<String> {
        params.get("session_id").and_then(|v| v.as_str()).map(String::from)
    }

    /// 解析一条请求:发送 JSON-RPC ok 回复 `{"choice":..}`(解锁 shim 阻塞),
    /// 记忆 + 审计 + 入 history。返回是否真解析了(pending 中存在该 id)。
    pub fn resolve(&mut self, ipc_id: u64, choice: FeedChoice, source: ResolveSource) -> bool {
        let Some(p) = self.pending.remove(&ipc_id) else {
            return false;
        };
        let resp = JsonRpcResponse::ok(ipc_id, json!({ "choice": choice.as_str() }));
        if let Some(tx) = p.reply.lock().unwrap().take() {
            // 解锁 listener.rs 的 reply_rx.await → 写回 socket → shim 的 read_line 返回。
            let _ = tx.send(resp);
        }
        if matches!(source, ResolveSource::User) {
            self.remember(&p.req.session_id, p.req.tool_name.as_deref().unwrap_or(""), choice);
        }
        append_audit(&p.req, choice, source);
        self.history.push((ipc_id, choice, source));
        if self.history.len() > 100 {
            self.history.remove(0);
        }
        if self.focused == Some(ipc_id) {
            self.focused = self.pending.keys().copied().next();
        }
        true
    }

    /// 扫描过期请求(超 `FEED_SOFT_TIMEOUT`)→ 返回需自动 Deny 的 ipc_id 列表。
    /// 顺便清过期的 bypass 记忆。
    pub fn expire_timeouts(&mut self) -> Vec<u64> {
        let now = Instant::now();
        let expired: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, p)| now.duration_since(p.req.created_at) >= FEED_SOFT_TIMEOUT)
            .map(|(id, _)| *id)
            .collect();
        self.bypass_until.retain(|_, until| *until > now);
        expired
    }

    /// 切换聚焦到下一条 pending(循环)。面板用 ↑/↓ 触发。
    pub fn focus_next(&mut self) {
        let mut ids: Vec<u64> = self.pending.keys().copied().collect();
        ids.sort_unstable();
        if ids.is_empty() {
            self.focused = None;
            return;
        }
        match self.focused {
            None => self.focused = Some(ids[0]),
            Some(cur) => {
                if let Some(pos) = ids.iter().position(|&x| x == cur) {
                    self.focused = Some(ids[(pos + 1) % ids.len()]);
                } else {
                    self.focused = Some(ids[0]);
                }
            }
        }
    }

    /// 当前聚焦的 pending 请求(展示用)。
    pub fn focused_request(&self) -> Option<&FeedRequestPayload> {
        self.focused.and_then(|id| self.pending.get(&id).map(|p| &p.req))
    }

    /// pending 总数(面板角标 / 页脚用)。
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// 面板是否可见(focused 非空)。
    pub fn is_visible(&self) -> bool {
        self.focused.is_some()
    }

    /// 倒计时秒数(聚焦请求距 120s 超时)。展示用。
    pub fn focused_remaining_secs(&self) -> Option<u64> {
        let req = self.focused_request()?;
        let elapsed = Instant::now().duration_since(req.created_at);
        FEED_SOFT_TIMEOUT.checked_sub(elapsed).map(|r| r.as_secs())
    }
}

// ── 审计日志 ──────────────────────────────────────────────────────────────

/// `data_local_dir()/pureterm/feed.jsonl`
/// (Linux `~/.local/share/pureterm/feed.jsonl`,macOS `~/Library/Application Support/...`)。
fn audit_path() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pureterm")
        .join("feed.jsonl")
}

fn append_audit(req: &FeedRequestPayload, choice: FeedChoice, source: ResolveSource) {
    use std::io::Write;
    let path = audit_path();
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let entry = json!({
        "ts": now_iso(),
        "ipc_id": req.ipc_id,
        "session_id": req.session_id,
        "event": req.event,
        "tool_name": req.tool_name,
        "subject": req.subject(),
        "choice": choice.as_str(),
        "source": match source {
            ResolveSource::User => "user",
            ResolveSource::Timeout => "timeout",
            ResolveSource::Memory => "memory",
        },
    });
    let mut line = match serde_json::to_string(&entry) {
        Ok(s) => s,
        Err(_) => return,
    };
    line.push('\n');
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    format!("T{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    fn payload(ipc_id: u64, session: &str, tool: &str) -> FeedRequestPayload {
        FeedRequestPayload {
            ipc_id,
            session_id: session.into(),
            event: "PreToolUse".into(),
            tool_name: Some(tool.into()),
            tool_input: Some(json!({"command":"ls"})),
            prompt: None,
            message: None,
            reason: "r".into(),
            cwd: None,
            created_at: Instant::now(),
        }
    }

    fn pending(req: FeedRequestPayload) -> PendingRequest {
        // reply 句柄置 None(resolve 测试不靠它;专门的 resolve 测试用真 oneshot)。
        PendingRequest { req, reply: Arc::new(Mutex::new(None)) }
    }

    #[test]
    fn consult_memory_precedence() {
        let mut st = FeedPanelState::default();
        st.always_remember.insert(("s1".into(), "Bash".into()), FeedChoice::Always);
        assert_eq!(st.consult_memory("s1", "Bash"), Some(FeedChoice::Always));
        assert_eq!(st.consult_memory("s1", "Write"), None);
        st.all_remember.insert("s1".into(), FeedChoice::All);
        assert_eq!(st.consult_memory("s1", "Write"), Some(FeedChoice::All));
        // bypass 未过期 > all
        st.bypass_until.insert("s1".into(), Instant::now() + BYPASS_WINDOW);
        assert_eq!(st.consult_memory("s1", "Write"), Some(FeedChoice::Bypass));
    }

    #[test]
    fn remember_records_correct_scope() {
        let mut st = FeedPanelState::default();
        st.remember("s1", "Bash", FeedChoice::Always);
        assert!(st.always_remember.contains_key(&("s1".into(), "Bash".into())));
        st.remember("s1", "Bash", FeedChoice::All);
        assert_eq!(st.all_remember.get("s1"), Some(&FeedChoice::All));
        st.remember("s1", "Bash", FeedChoice::Bypass);
        assert!(st.bypass_until.contains_key("s1"));
        st.remember("s1", "Bash", FeedChoice::Once);
        assert!(!st.always_remember.contains_key(&("s1".into(), "Bash".into())) || st.all_remember.contains_key("s1"));
    }

    #[test]
    fn clear_session_drops_memory() {
        let mut st = FeedPanelState::default();
        st.always_remember.insert(("s1".into(), "Bash".into()), FeedChoice::Always);
        st.all_remember.insert("s1".into(), FeedChoice::All);
        st.bypass_until.insert("s1".into(), Instant::now() + BYPASS_WINDOW);
        st.clear_session("s1");
        assert!(st.always_remember.is_empty());
        assert!(st.all_remember.is_empty());
        assert!(st.bypass_until.is_empty());
    }

    #[test]
    fn parse_payload_requires_session_and_event() {
        let p = FeedPanelState::parse_payload(7, &json!({"session_id":"s","event":"PreToolUse","tool_name":"Bash"}));
        assert!(p.is_some());
        let pp = p.unwrap();
        assert_eq!(pp.ipc_id, 7);
        assert_eq!(pp.tool_name.as_deref(), Some("Bash"));
        assert!(FeedPanelState::parse_payload(7, &json!({"event":"PreToolUse"})).is_none());
    }

    #[test]
    fn resolve_sends_on_oneshot_and_remembers() {
        let mut st = FeedPanelState::default();
        let (tx, rx) = oneshot::channel::<JsonRpcResponse>();
        let req = payload(42, "s1", "Bash");
        st.pending.insert(42, PendingRequest { req: req.clone(), reply: Arc::new(Mutex::new(Some(tx))) });
        assert!(st.resolve(42, FeedChoice::Always, ResolveSource::User));
        let resp = rx.blocking_recv().expect("reply sent");
        assert_eq!(resp.result.unwrap()["choice"], "always");
        assert!(st.always_remember.contains_key(&("s1".into(), "Bash".into())));
        assert_eq!(st.history.len(), 1);
        assert!(st.pending.is_empty());
    }

    #[test]
    fn resolve_missing_id_is_noop() {
        let mut st = FeedPanelState::default();
        assert!(!st.resolve(999, FeedChoice::Deny, ResolveSource::Timeout));
    }

    #[test]
    fn expire_timeouts_returns_only_old() {
        let mut st = FeedPanelState::default();
        let mut old = payload(1, "s", "Bash");
        old.created_at = Instant::now() - Duration::from_secs(200);
        st.pending.insert(1, pending(old));
        st.pending.insert(2, pending(payload(2, "s", "Bash"))); // fresh
        let expired = st.expire_timeouts();
        assert_eq!(expired, vec![1]);
    }

    #[test]
    fn focus_next_cycles() {
        let mut st = FeedPanelState::default();
        st.pending.insert(10, pending(payload(10, "s", "Bash")));
        st.pending.insert(20, pending(payload(20, "s", "Bash")));
        st.focus_next();
        assert_eq!(st.focused, Some(10));
        st.focus_next();
        assert_eq!(st.focused, Some(20));
        st.focus_next();
        assert_eq!(st.focused, Some(10));
    }

    #[test]
    fn focused_remaining_secs_decreases_within_window() {
        let mut st = FeedPanelState::default();
        let mut req = payload(1, "s", "Bash");
        req.created_at = Instant::now() - Duration::from_secs(10);
        st.pending.insert(1, pending(req));
        st.focused = Some(1);
        let r = st.focused_remaining_secs().unwrap();
        assert!(r <= 110 && r >= 108, "expected ~109s, got {r}");
    }
}
