//! `feed.*` / `resume.*` / `agent.*` / `events.*` / `notification.*` 命名空间方法注册。
//!
//! 这里集中注册与 AI agent / 事件日志 / 通知相关的 fire-and-forget 方法
//! (绝大多数都是立即回复 + 携带副作用,真实工作发生在主循环)。Feed / Resume /
//! Agent 在非 unix 平台注册为 noop 桩。由 [`super::SocketRouter::new`] 调用
//! [`SocketRouter::register_agent`] 收集。

use serde_json::Value;

use super::types::IpcSideEffect;
use super::SocketRouter;

impl SocketRouter {
    /// 注册 feed.* / resume.* / agent.* / events.* / notification.* 方法。
    pub(super) fn register_agent(&mut self) {
        // ── feed.* (GAP-7.4, 2 methods) ───────────────────────────────
        // `feed.request_approval`: 正常路径在 `Message::Ipc` 的 deferred-reply
        // 预判分支里(不立即回复,等人在 Feed 面板点按钮)。这里仅作注册,让
        // `system.methods` 列出它,并提供兜底(若被绕过预判直接 dispatch)。
        #[cfg(unix)]
        {
            self.register("feed.request_approval", |_params, _snap| {
                (Ok(serde_json::json!({"queued": true})), None)
            });
            // `feed.clear_session`: 立即回复 + 副作用(SessionEnd 清记忆)。
            self.register("feed.clear_session", |params, _snap| {
                let session_id = params
                    .as_ref()
                    .and_then(|p| p.get("session_id").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                (
                    Ok(serde_json::json!({"ok": true, "session_id": session_id})),
                    Some(IpcSideEffect::FeedClearSession { session_id }),
                )
            });
            // GAP-7.7: `resume.request_approval` — like feed.request_approval,
            // the real work happens in the `Message::Ipc` deferred-reply branch
            // (token check / Feed panel / spawn). Registered here so
            // `system.methods` lists it and as a dispatch fallback.
            self.register("resume.request_approval", |_params, _snap| {
                (Ok(serde_json::json!({"queued": true})), None)
            });
        }
        #[cfg(not(unix))]
        {
            self.register_noop("feed.request_approval", "Feed not available on this platform");
            self.register_noop("feed.clear_session", "Feed not available on this platform");
            self.register_noop("resume.request_approval", "Resume not available on this platform");
        }

        // ── agent.* (GAP-7.5, 1 method) ─────────────────────────────
        // `agent.session_started`: SessionStart hook fire-and-forget 通知
        // (shim → GUI) → 持久化 session id + 关联 pane(供冷恢复)。立即回复
        // `{"ok": true}` + 副作用。
        #[cfg(unix)]
        {
            self.register("agent.session_started", |params, _snap| {
                let g = |key: &str| {
                    params
                        .as_ref()
                        .and_then(|p| p.get(key).and_then(|v| v.as_str()))
                        .map(|s| s.to_string())
                };
                let session_id = g("session_id").unwrap_or_default();
                let kind = g("kind");
                let cwd = g("cwd");
                (
                    Ok(serde_json::json!({"ok": true, "session_id": session_id})),
                    Some(IpcSideEffect::AgentSessionStarted { session_id, kind, cwd }),
                )
            });
        }
        #[cfg(not(unix))]
        {
            self.register_noop("agent.session_started", "Agent sessions not available on this platform");
        }

        // ── events.* (GAP-6.7, 1 method) ─────────────────────────────
        // `events.recent`: one-shot read of recent events from events.jsonl.
        // The file is the real-time source (CLI `pureterm events tail` tails
        // it); this method is the programmatic / `pureterm msg` entry point.
        // Cross-platform — pure file read, no side effect.
        self.register("events.recent", |params, _snap| {
            let g = |key: &str| {
                params.as_ref().and_then(|p| p.get(key))
            };
            let filter_raw = g("type").and_then(|v| v.as_str());
            let limit = g("limit").and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or(50);
            let since_seq = g("since_seq").and_then(|v| v.as_u64());
            let filter = crate::events::TypeFilter::parse(filter_raw);
            let entries = crate::events_log::read_recent(&filter, limit, since_seq);
            let arr: Vec<Value> = entries
                .into_iter()
                .map(|e| serde_json::to_value(&e).unwrap_or(Value::Null))
                .collect();
            (Ok(serde_json::json!({ "events": arr })), None)
        });

        // ── notification.* (GAP-09, 1 method) ────────────────────────
        // `notification.post`: CLI `pureterm notify` / external caller → post a
        // notification into the active (or specified) pane. Fire-and-forget;
        // replied immediately and the main loop does the store/toast work.
        self.register("notification.post", |params, _snap| {
            let g = |key: &str| params.as_ref().and_then(|p| p.get(key));
            let body = match g("body").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => {
                    return (
                        Ok(serde_json::json!({ "posted": false, "reason": "empty body" })),
                        None,
                    );
                }
            };
            let title = g("title").and_then(|v| v.as_str()).map(|s| s.to_string());
            let pane_id = g("pane_id").and_then(|v| v.as_u64());
            (
                Ok(serde_json::json!({ "posted": true })),
                Some(IpcSideEffect::ShowNotification { title, body, pane_id }),
            )
        });
    }
}
