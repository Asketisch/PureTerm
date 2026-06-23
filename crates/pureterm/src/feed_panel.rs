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
    /// GAP-7.7: present when this is a resume-binding approval (not a tool hook).
    /// On Always, the GUI mints+persists an HMAC token.
    pub resume: Option<ResumePayload>,
}

/// GAP-7.7: resume-binding approval payload. On Always, `resolve()` mints an
/// HMAC token over `(kind, checkpoint, shell)` via `resume_bindings::approve_token`.
#[derive(Debug, Clone)]
pub struct ResumePayload {
    pub kind: String,
    pub checkpoint: String,
    pub shell: Vec<String>,
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

/// `FeedPanelState::accept_inbound` 的产出 —— iced 侧 `handle_feed_request_approval`
/// 的纯核。携带 iced 调用方完成它独占的两个副作用(emit `feed.*` 事件、spawn resume
/// tab)所需的一切,逻辑本身不碰 `&mut App`、不碰事件总线、不 spawn。
pub struct InboundDisposition {
    pub ipc_id: u64,
    pub session_id: String,
    /// `feed.requested` 事件的 JSON(ErrorReplied 时调用方不发)。
    pub request_meta: Value,
    pub outcome: InboundOutcome,
}

/// 入站请求的处理结果。
pub enum InboundOutcome {
    /// params 缺失/非法 —— err 已经经 reply 句柄发回,调用方无需再做。
    ErrorReplied,
    /// 无记忆命中 —— 面板已聚焦,等用户决定或 120s 超时。
    Focused,
    /// 记忆命中(Always/All/Bypass 或 resume token)—— 已 resolve(回复已发、记忆/审计已落)。
    /// 调用方 emit `feed.resolved`;若 `resume` 为 Some,还要 spawn resume tab。
    Resolved { choice: FeedChoice, resume: Option<ResumePayload> },
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
    bypass_until: HashMap<String, Instant>,    // session_id → 过期时刻
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
        if let Some(&c) = self.always_remember.get(&(session_id.to_string(), tool_name.to_string()))
        {
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
        // GAP-7.7: a resume-binding approval carries kind+checkpoint+shell.
        let resume = (|| {
            let kind = s.get("kind")?.as_str()?.to_string();
            let checkpoint = s.get("checkpoint")?.as_str()?.to_string();
            let shell = s
                .get("shell")?
                .as_array()?
                .iter()
                .map(|v| v.as_str().unwrap_or("").to_string())
                .collect::<Vec<_>>();
            Some(ResumePayload { kind, checkpoint, shell })
        })();
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
            resume,
        })
    }

    /// `handle_feed_request_approval`(iced 侧)的纯核。解析 params → 记忆咨询
    /// (in-map Always/All/Bypass,或对 resume payload 调注入的 `resume_authorized`)
    /// → stash PendingRequest → 命中则即时 resolve(发 oneshot 回复 + remember/审计)
    /// 或聚焦面板。返回 `InboundDisposition` 让 iced 调用方做它独占的两个副作用
    /// (emit 事件 + spawn resume tab)。
    ///
    /// `resume_authorized` 注入是为了保持纯 + 可测:不直接碰 `resume_bindings` 的
    /// 落盘 token 存储;iced 调用方传真实的 `resume_bindings::authorize` 判定。
    pub fn accept_inbound(
        &mut self,
        ipc_id: u64,
        params: Option<&Value>,
        reply: ReplyHandle,
        resume_authorized: impl Fn(&ResumePayload) -> bool,
    ) -> InboundDisposition {
        use crate::ipc::protocol::JsonRpcResponse;

        // 缺 params → -32602 err,直接回复,其余不做。
        let Some(params) = params else {
            let resp =
                JsonRpcResponse::err(ipc_id, -32602, "feed.request_approval requires params");
            if let Some(tx) = reply.lock().unwrap().take() {
                let _ = tx.send(resp);
            }
            return InboundDisposition {
                ipc_id,
                session_id: String::new(),
                request_meta: Value::Null,
                outcome: InboundOutcome::ErrorReplied,
            };
        };
        // 非法 payload → -32602 err。
        let Some(payload) = Self::parse_payload(ipc_id, params) else {
            let resp = JsonRpcResponse::err(
                ipc_id,
                -32602,
                "feed.request_approval requires session_id + event",
            );
            if let Some(tx) = reply.lock().unwrap().take() {
                let _ = tx.send(resp);
            }
            return InboundDisposition {
                ipc_id,
                session_id: String::new(),
                request_meta: Value::Null,
                outcome: InboundOutcome::ErrorReplied,
            };
        };

        // 记忆咨询:resume payload → 注入的 token 判定;否则 in-map。
        let memory_choice = if let Some(r) = payload.resume.as_ref() {
            if resume_authorized(r) {
                Some(FeedChoice::Always)
            } else {
                None
            }
        } else {
            self.consult_memory(&payload.session_id, payload.tool_name.as_deref().unwrap_or(""))
        };

        let session_id = payload.session_id.clone();
        let request_meta = json!({
            "ipc_id": payload.ipc_id,
            "event": payload.event,
            "tool_name": payload.tool_name,
            "subject": payload.subject(),
        });

        // stash PendingRequest(reply 句柄随之移交,后续 resolve/timeout 取用)。
        self.pending.insert(ipc_id, PendingRequest { req: payload, reply });

        let outcome = if let Some(choice) = memory_choice {
            // GAP-7.7: resolve 会 drain pending,先抓出 resume 供调用方 spawn。
            let resume = self.pending.get(&ipc_id).and_then(|p| p.req.resume.clone());
            self.resolve(ipc_id, choice, ResolveSource::Memory);
            InboundOutcome::Resolved { choice, resume }
        } else {
            self.focused = Some(ipc_id);
            InboundOutcome::Focused
        };

        InboundDisposition { ipc_id, session_id, request_meta, outcome }
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
            if let Some(resume) = &p.req.resume {
                // GAP-7.7: resume approval — on Always, mint+persist the HMAC
                // token (the persistent memory). Resume approvals deliberately
                // bypass the in-memory always_remember map: its (session_id,
                // tool_name) key would collide on ("", "").
                if matches!(choice, FeedChoice::Always) {
                    crate::resume_bindings::approve_token(
                        &resume.kind,
                        &resume.checkpoint,
                        &resume.shell,
                    );
                }
            } else {
                self.remember(&p.req.session_id, p.req.tool_name.as_deref().unwrap_or(""), choice);
            }
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
///
/// `PURETERM_FEED_LOG` 覆盖(GAP-7.4 E2E harness 用):设了就用该路径,方便测试在
/// temp dir 断言审计行而不污染用户真实 feed.jsonl。与 `feed_log::audit_path` 同语义。
fn audit_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("PURETERM_FEED_LOG") {
        return std::path::PathBuf::from(p);
    }
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("pureterm").join("feed.jsonl")
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

// `PURETERM_FEED_LOG` is a single process-global env key. Serialize every test
// that mutates it (feed_panel, feed_log, agent_e2e) through this one lock so
// they can't race. Referenced as `crate::feed_panel::FEED_AUDIT_ENV_LOCK`.
#[cfg(test)]
pub(crate) static FEED_AUDIT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[test]
    fn audit_path_respects_env_override() {
        let _guard = super::FEED_AUDIT_ENV_LOCK.lock().unwrap();
        let override_path = std::env::temp_dir().join("pureterm_test_feed_audit_override.jsonl");
        std::env::set_var("PURETERM_FEED_LOG", &override_path);
        // GAP-7.4 E2E harness: audit_path() must honor PURETERM_FEED_LOG so the
        // round-trip test can assert audit rows in a temp dir without polluting
        // the user's real feed.jsonl.
        assert_eq!(audit_path(), override_path);
        std::env::remove_var("PURETERM_FEED_LOG");
    }

    // ── accept_inbound (GAP-7.4 extracted pure core of handle_feed_request_approval) ──
    fn reply_handle() -> (ReplyHandle, oneshot::Receiver<JsonRpcResponse>) {
        let (tx, rx) = oneshot::channel();
        (Arc::new(Mutex::new(Some(tx))), rx)
    }
    fn tool_params(session: &str, tool: &str) -> Value {
        json!({"session_id": session, "event": "PreToolUse", "tool_name": tool, "tool_input": {"command":"ls"}})
    }
    fn resume_params(session: &str, kind: &str, checkpoint: &str, shell: &[&str]) -> Value {
        json!({"session_id": session, "event": "PreToolUse", "kind": kind, "checkpoint": checkpoint, "shell": shell})
    }

    #[test]
    fn accept_inbound_missing_params_replies_err() {
        let mut st = FeedPanelState::default();
        let (reply, rx) = reply_handle();
        let d = st.accept_inbound(7, None, reply, |_| false);
        assert!(matches!(d.outcome, InboundOutcome::ErrorReplied));
        let resp = rx.blocking_recv().expect("err replied");
        assert!(resp.error.is_some(), "expected -32602 error");
        assert!(st.pending.is_empty());
    }

    #[test]
    fn accept_inbound_tool_no_memory_focuses_panel() {
        let mut st = FeedPanelState::default();
        let (reply, _rx) = reply_handle();
        let d = st.accept_inbound(1, Some(&tool_params("s1", "Bash")), reply, |_| false);
        assert!(matches!(d.outcome, InboundOutcome::Focused));
        assert_eq!(st.focused, Some(1));
        assert_eq!(st.pending.len(), 1);
    }

    #[test]
    fn accept_inbound_memory_hit_resolves_immediately() {
        let mut st = FeedPanelState::default();
        st.remember("s1", "Bash", FeedChoice::Always);
        let (reply, rx) = reply_handle();
        let d = st.accept_inbound(1, Some(&tool_params("s1", "Bash")), reply, |_| false);
        match d.outcome {
            InboundOutcome::Resolved { choice, resume } => {
                assert_eq!(choice, FeedChoice::Always);
                assert!(resume.is_none());
            }
            _ => panic!("expected Resolved"),
        }
        let resp = rx.blocking_recv().expect("reply sent");
        assert_eq!(resp.result.unwrap()["choice"], "always");
        assert!(st.pending.is_empty(), "resolved drains pending");
    }

    #[test]
    fn accept_inbound_resume_token_preapproved_resolves_always() {
        let mut st = FeedPanelState::default();
        let (reply, rx) = reply_handle();
        let p = resume_params("s1", "tmux", "work", &["tmux", "attach", "-t", "work"]);
        // closure = "token pre-approved" → memory hit → resolve Always + carry resume.
        let d = st.accept_inbound(1, Some(&p), reply, |_| true);
        match d.outcome {
            InboundOutcome::Resolved { choice, resume } => {
                assert_eq!(choice, FeedChoice::Always);
                let r = resume.expect("resume payload carried for tab spawn");
                assert_eq!(r.kind, "tmux");
                assert_eq!(r.shell, vec!["tmux", "attach", "-t", "work"]);
            }
            _ => panic!("expected Resolved"),
        }
        let resp = rx.blocking_recv().expect("reply sent");
        assert_eq!(resp.result.unwrap()["choice"], "always");
    }

    #[test]
    fn accept_inbound_resume_no_token_focuses() {
        let mut st = FeedPanelState::default();
        let (reply, _rx) = reply_handle();
        let p = resume_params("s1", "tmux", "work", &["tmux", "attach"]);
        // closure = "no token" → not pre-approved → focus panel, no reply yet.
        let d = st.accept_inbound(1, Some(&p), reply, |_| false);
        assert!(matches!(d.outcome, InboundOutcome::Focused));
        assert_eq!(st.focused, Some(1));
    }

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
            resume: None,
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
        assert!(
            !st.always_remember.contains_key(&("s1".into(), "Bash".into()))
                || st.all_remember.contains_key("s1")
        );
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
        let p = FeedPanelState::parse_payload(
            7,
            &json!({"session_id":"s","event":"PreToolUse","tool_name":"Bash"}),
        );
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
        st.pending
            .insert(42, PendingRequest { req: req.clone(), reply: Arc::new(Mutex::new(Some(tx))) });
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
