//! GAP-7.3:休眠确认面板 —— GUI 侧状态。
//!
//! 当 `HibernationScanTick`(iced_app)发现满足双重空闲条件(PTY 静默
//! 且 CPU 持续低于阈值,持续达到配置时长)且存活会话数超过
//! `max_live_terminals` 时,把一个 `PendingHibernation` 推到这里。右侧置顶
//! 浮层显示它并带倒计时;用户确认(→ `agent.hibernate` SIGSTOP)或取消。
//! 超时自动确认。同一时间只显示一条确认 —— 若当前正在处理,忙碌的扫描
//! 在本次结束后于下一 tick 重新评估。

use std::time::Instant;

/// 一条待处理的休眠确认。
#[derive(Debug, Clone)]
pub struct PendingHibernation {
    /// 确认后要 SIGSTOP 的 daemon session id。
    pub session_id: String,
    /// 用于展示的 PTY/agent 类型标签("local" / "claude-code" / …)。
    pub kind: String,
    /// 确认发起的时间(驱动倒计时)。
    pub started_at: Instant,
    /// 超过该秒数后自动确认(来自配置,默认 60)。
    pub confirm_secs: u64,
}

/// 休眠面板状态。应用始终持有它;可见性 =
/// `pending.is_some()`。形状与 `feed_panel::FeedPanelState` 一致,但只保存
/// 一条待确认(休眠一次一条,与 Feed 的队列不同),且无批准记忆。
pub struct HibernationPanelState {
    pub pending: Option<PendingHibernation>,
}

impl Default for HibernationPanelState {
    fn default() -> Self {
        Self { pending: None }
    }
}

impl HibernationPanelState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 浮层面板是否应渲染。
    pub fn is_visible(&self) -> bool {
        self.pending.is_some()
    }

    /// 发起一条确认。同一时间只允许一条;若已有待处理,新请求被丢弃
    /// (扫描会在下一 tick 重新评估)。
    pub fn request(&mut self, p: PendingHibernation) {
        if self.pending.is_none() {
            self.pending = Some(p);
        }
    }

    /// 用户确认 → 取出要休眠的 session id(并清空面板)。
    pub fn confirm(&mut self) -> Option<String> {
        self.pending.take().map(|p| p.session_id)
    }

    /// 用户取消 → 清空且不休眠。
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// 若确认窗口已过则自动确认。超时触发时返回要休眠的 session id。
    pub fn expire_timeout(&mut self) -> Option<String> {
        let p = self.pending.as_ref()?;
        if p.started_at.elapsed().as_secs() >= p.confirm_secs {
            self.pending.take().map(|p| p.session_id)
        } else {
            None
        }
    }

    /// 倒计时剩余秒数(用于展示);无面板时为 `None`。
    pub fn remaining_secs(&self) -> Option<u64> {
        self.pending
            .as_ref()
            .map(|p| p.confirm_secs.saturating_sub(p.started_at.elapsed().as_secs()))
    }

    /// 待处理的 session id(若有,用于展示 / 状态栏)。
    pub fn focused_session_id(&self) -> Option<&str> {
        self.pending.as_ref().map(|p| p.session_id.as_str())
    }
}

/// GAP-7.6: `kind` 是否在 `[[vault.agents]]` 中标记为 `never_hibernate`
/// (豁免 hibernation 候选 + 不计入 live-agent)。纯函数,便于单测。
pub(crate) fn vault_kind_never_hibernate(
    vault_agents: &[crate::app_config::VaultAgentEntry],
    kind: &str,
) -> bool {
    vault_agents.iter().find(|v| v.kind == kind).map(|v| v.never_hibernate).unwrap_or(false)
}

/// GAP-7.3: `hibernation_scan` 的纯核。给定 daemon session 快照(每条含
/// `id`/`agent_kind`/`hibernated`/`pty_idle_secs`/`cpu_idle_secs`)+ cfg + vault 等,
/// 决定是否提示 hibernation。当 live-agent 数 > `max_live_terminals` 且存在非豁免
/// idle 候选时,返回该候选(`pty_idle_secs` 最长者);否则 None。IPC 取数 / 缓存 /
/// 面板 request 留给 iced 调用方。
pub fn classify_candidate(
    sessions: &[serde_json::Value],
    cfg: &crate::app_config::AgentHibernationConfig,
    vault: &[crate::app_config::VaultAgentEntry],
) -> Option<(String, String)> {
    let is_idle = |s: &serde_json::Value| -> bool {
        if s.get("hibernated").and_then(|v| v.as_bool()).unwrap_or(false) {
            return false;
        }
        if s.get("agent_kind")
            .and_then(|v| v.as_str())
            .map(|k| vault_kind_never_hibernate(vault, k))
            .unwrap_or(false)
        {
            return false;
        }
        let pty = s.get("pty_idle_secs").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);
        let cpu = s.get("cpu_idle_secs").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);
        pty >= cfg.pty_idle_secs && cpu >= cfg.cpu_idle_secs
    };
    let is_live_agent = |s: &serde_json::Value| {
        s.get("hibernated").and_then(|v| v.as_bool()) == Some(false)
            && s.get("agent_kind")
                .and_then(|v| v.as_str())
                .map(|k| {
                    if vault_kind_never_hibernate(vault, k) {
                        return false;
                    }
                    k == "claude-code" || vault.iter().any(|v| v.kind == k)
                })
                .unwrap_or(false)
    };

    // 未超 max_live_terminals → 不提示。
    if sessions.iter().filter(|s| is_live_agent(s)).count() <= cfg.max_live_terminals as usize {
        return None;
    }
    // 在 idle 候选中选 pty_idle 最长者。
    let best = sessions
        .iter()
        .filter(|s| is_idle(s))
        .max_by_key(|s| s.get("pty_idle_secs").and_then(|v| v.as_u64()).unwrap_or(0))?;
    let session_id = best.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if session_id.is_empty() {
        return None;
    }
    let kind = best
        .get("agent_kind")
        .and_then(|v| v.as_str())
        .or_else(|| best.get("kind").and_then(|v| v.as_str()))
        .unwrap_or("local")
        .to_string();
    Some((session_id, kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::{AgentHibernationConfig, VaultAgentEntry};
    use serde_json::{json, Value};
    use std::time::Duration;

    fn pending(confirm_secs: u64) -> PendingHibernation {
        PendingHibernation {
            session_id: "s1".into(),
            kind: "claude-code".into(),
            started_at: Instant::now(),
            confirm_secs,
        }
    }

    #[test]
    fn request_then_confirm_returns_session_id() {
        let mut st = HibernationPanelState::new();
        assert!(!st.is_visible());
        st.request(pending(60));
        assert!(st.is_visible());
        assert_eq!(st.confirm(), Some("s1".to_string()));
        assert!(!st.is_visible());
    }

    #[test]
    fn cancel_clears_without_hibernating() {
        let mut st = HibernationPanelState::new();
        st.request(pending(60));
        st.cancel();
        assert!(st.confirm().is_none());
    }

    #[test]
    fn second_request_while_pending_is_dropped() {
        let mut st = HibernationPanelState::new();
        st.request(pending(60));
        let mut p2 = pending(60);
        p2.session_id = "s2".into();
        st.request(p2);
        // 先到的待处理优先;第二条被丢弃。
        assert_eq!(st.focused_session_id(), Some("s1"));
    }

    #[test]
    fn expire_timeout_fires_after_window() {
        let mut st = HibernationPanelState::new();
        let mut p = pending(60);
        p.started_at = Instant::now() - Duration::from_secs(61);
        st.pending = Some(p);
        assert_eq!(st.expire_timeout(), Some("s1".to_string()));
        assert!(!st.is_visible());
    }

    #[test]
    fn expire_timeout_none_within_window() {
        let mut st = HibernationPanelState::new();
        st.request(pending(60));
        assert!(st.expire_timeout().is_none());
        assert!(st.is_visible());
    }

    // ── classify_candidate / vault_kind_never_hibernate(GAP-7.3 抽出的纯核) ──
    fn cfg(max_live: u32, idle: u64) -> AgentHibernationConfig {
        AgentHibernationConfig {
            enabled: true,
            max_live_terminals: max_live,
            pty_idle_secs: idle,
            cpu_idle_secs: idle,
            confirm_secs: 60,
            scan_interval_secs: 3,
            cpu_sample_interval_secs: 1,
        }
    }
    fn ventry(kind: &str, never: bool) -> VaultAgentEntry {
        VaultAgentEntry { kind: kind.into(), never_hibernate: never, ..Default::default() }
    }
    fn live_sess(id: &str, kind: &str, pty: u64, cpu: u64) -> Value {
        json!({"id": id, "agent_kind": kind, "hibernated": false, "pty_idle_secs": pty, "cpu_idle_secs": cpu})
    }

    #[test]
    fn vault_kind_never_hibernate_flag() {
        // 从 iced_app 迁移:never_hibernate=true → 豁免;false/未登记 → 不豁免。
        let agents = vec![ventry("claude-code", false), ventry("codex", true)];
        assert!(!vault_kind_never_hibernate(&agents, "claude-code"));
        assert!(vault_kind_never_hibernate(&agents, "codex"));
    }

    #[test]
    fn vault_kind_never_hibernate_unknown_kind_is_false() {
        let agents = vec![ventry("claude-code", true)];
        assert!(!vault_kind_never_hibernate(&agents, "unknown"));
        assert!(!vault_kind_never_hibernate(&[], "claude-code"));
    }

    #[test]
    fn classify_candidate_picks_longest_idle_live_agent() {
        let sessions = vec![
            live_sess("a", "claude-code", 100, 100), // idle,最长
            live_sess("b", "claude-code", 1, 1),    // 活跃
            live_sess("c", "claude-code", 50, 50),  // idle,较短
        ];
        // 3 个存活 agent > max 2 → 触发;在 idle 候选 (a, c) 中选 pty_idle 最大者 → a。
        assert_eq!(
            classify_candidate(&sessions, &cfg(2, 5), &vec![]),
            Some(("a".to_string(), "claude-code".to_string()))
        );
    }

    #[test]
    fn classify_candidate_none_when_live_count_at_or_below_max() {
        let sessions = vec![live_sess("a", "claude-code", 100, 100)];
        // 1 个存活 <= max 5 → 不提示。
        assert_eq!(classify_candidate(&sessions, &cfg(5, 5), &vec![]), None);
    }

    #[test]
    fn classify_candidate_skips_never_hibernate_idle_candidate() {
        let vault = vec![ventry("precious", true)];
        let sessions = vec![
            live_sess("live1", "claude-code", 1, 1), // 存活且活跃(计入触发)
            live_sess("live2", "claude-code", 1, 1), // 存活且活跃
            live_sess("keep", "precious", 999, 999), // idle 但 never_hibernate → 跳过
        ];
        // 2 个存活(claude-code) > max 1 → 触发;idle 候选:"keep" 已豁免 → 无。
        assert_eq!(classify_candidate(&sessions, &cfg(1, 5), &vault), None);
    }

    #[test]
    fn classify_candidate_skips_already_hibernated() {
        let mut sessions = vec![
            live_sess("h", "claude-code", 999, 999),
            live_sess("live1", "claude-code", 1, 1),
            live_sess("live2", "claude-code", 1, 1),
        ];
        sessions[0]["hibernated"] = json!(true);
        // 2 个存活(live1, live2;"h" 已休眠 → 不算存活也不算 idle 候选) > max 1 →
        // 触发,但没有非豁免的 idle 候选 → None。
        assert_eq!(classify_candidate(&sessions, &cfg(1, 5), &vec![]), None);
    }
}
