//! GAP-7.6: detect-resume 提示面板 —— GUI 侧状态机。
//!
//! 当 `DetectScanTick`(iced_app)运行 `agent_detect::scan` 发现运行中的
//! agent 进程、其 kind 带有已持久化的 `session_id` 时,结果会
//! 推送到这里。右侧固定的浮层展示它们;用户可恢复其中一个
//! 或全部(→ `create_resume_tab` 在新窗格中拉起 `claude --resume <id>`)
//! 或全部忽略。已忽略/已恢复的 session_id 会在进程存活期内被抑制
//! (检测只是*提示*,不是关键路径,因此
//! 重启后再次提示一次是可接受的)。
//!
//! 形状上镜像 `hibernate_panel::HibernationPanelState`,但持有批
//! (`Vec`)而非单条确认,并跟踪已忽略/已恢复的 id。

use std::collections::HashSet;

use crate::agent_detect::DetectedAgent;

/// 一个可恢复的已检测 agent(无 session_id 的已被滤除、已去重)。
#[derive(Debug, Clone)]
pub struct DetectedResumeEntry {
    /// 匹配的 `[[vault.agents]]` kind(如 "claude-code")。
    pub kind: String,
    /// 传给恢复命令的已持久化 agent 会话 id。
    pub session_id: String,
    /// 匹配进程的主机 pid(仅展示)。
    pub pid: u32,
}

/// 检测-恢复面板状态。始终存在于应用;可见性 =
/// `!entries.is_empty()`。
pub struct DetectPanelState {
    pub entries: Vec<DetectedResumeEntry>,
    dismissed: HashSet<String>,
    resumed: HashSet<String>,
}

impl Default for DetectPanelState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            dismissed: HashSet::new(),
            resumed: HashSet::new(),
        }
    }
}

impl DetectPanelState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 浮层面板是否应该渲染。
    pub fn is_visible(&self) -> bool {
        !self.entries.is_empty()
    }

    /// 从一次扫描结果(`agent_detect::with_sessions` 输出)刷新:丢弃
    /// 无 session_id 的条目,跳过已忽略/已恢复的,按 session_id 去重。
    /// `entries` 始终反映*最新*一次扫描(真相是
    /// "当前运行中、未忽略/未恢复且带 session_id 的 agent"),因而
    /// 退出的进程会消失、新启动的会出现。扫描是
    /// 确定性的,约每 30s 运行一次,抖动风险可忽略,
    /// 恢复/忽略过滤在下一个 tick 生效。
    pub fn refresh(&mut self, detected: Vec<(DetectedAgent, Option<String>)>) {
        let mut seen: HashSet<String> = HashSet::new();
        let mut new_entries = Vec::new();
        for (d, sid) in detected {
            let Some(sid) = sid else { continue };
            if self.dismissed.contains(&sid) || self.resumed.contains(&sid) {
                continue;
            }
            if !seen.insert(sid.clone()) {
                continue;
            }
            new_entries.push(DetectedResumeEntry {
                kind: d.kind,
                session_id: sid,
                pid: d.pid,
            });
        }
        self.entries = new_entries;
    }

    /// 用户选择某一条目 → 返回 `(kind, session_id)`,记录为已恢复,
    /// 并将其从批中移除。
    pub fn resume_one(&mut self, session_id: &str) -> Option<(String, String)> {
        let pos = self
            .entries
            .iter()
            .position(|e| e.session_id == session_id)?;
        let e = self.entries.remove(pos);
        self.resumed.insert(e.session_id.clone());
        Some((e.kind, e.session_id))
    }

    /// 用户选择"全部恢复" → 返回所有 `(kind, session_id)`,记录
    /// 全部为已恢复,并清空批。
    pub fn resume_all(&mut self) -> Vec<(String, String)> {
        let out: Vec<(String, String)> = self
            .entries
            .iter()
            .map(|e| (e.kind.clone(), e.session_id.clone()))
            .collect();
        for e in &self.entries {
            self.resumed.insert(e.session_id.clone());
        }
        self.entries.clear();
        out
    }

    /// 用户忽略了整批("稍后" / ×) → 记录当前所有 session_id
    /// 为已忽略,并清空批。
    pub fn dismiss_all(&mut self) {
        for e in &self.entries {
            self.dismissed.insert(e.session_id.clone());
        }
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detected(kind: &str, pid: u32) -> DetectedAgent {
        DetectedAgent {
            kind: kind.into(),
            pid,
            cmdline: format!("{kind} --pid {pid}"),
        }
    }

    fn with_sid(kind: &str, pid: u32, sid: Option<&str>) -> (DetectedAgent, Option<String>) {
        (detected(kind, pid), sid.map(String::from))
    }

    #[test]
    fn is_visible_tracks_entries() {
        let mut st = DetectPanelState::new();
        assert!(!st.is_visible());
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        assert!(st.is_visible());
        st.resume_one("sid-a");
        assert!(!st.is_visible());
    }

    #[test]
    fn refresh_filters_out_none_session_id() {
        let mut st = DetectPanelState::new();
        st.refresh(vec![
            with_sid("claude-code", 1, None),
            with_sid("claude-code", 2, Some("sid-2")),
        ]);
        assert_eq!(st.entries.len(), 1);
        assert_eq!(st.entries[0].session_id, "sid-2");
    }

    #[test]
    fn refresh_dedups_same_session_id() {
        // 两个主机 pid 引用到同一 kind 的最新 session_id。
        let mut st = DetectPanelState::new();
        st.refresh(vec![
            with_sid("claude-code", 1, Some("sid-x")),
            with_sid("claude-code", 2, Some("sid-x")),
        ]);
        assert_eq!(st.entries.len(), 1);
    }

    #[test]
    fn refresh_skips_dismissed_session_id() {
        let mut st = DetectPanelState::new();
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-d"))]);
        st.dismiss_all();
        assert!(!st.is_visible());
        // 重新扫描:sid-d 已被忽略 → 跳过;sid-e 仍会浮现。
        st.refresh(vec![
            with_sid("claude-code", 1, Some("sid-d")),
            with_sid("claude-code", 2, Some("sid-e")),
        ]);
        assert_eq!(st.entries.len(), 1);
        assert_eq!(st.entries[0].session_id, "sid-e");
    }

    #[test]
    fn refresh_skips_resumed_session_id() {
        let mut st = DetectPanelState::new();
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        st.resume_one("sid-a");
        // 重新扫描:sid-a 已恢复 → 跳过。
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        assert!(!st.is_visible());
    }

    #[test]
    fn refresh_recomputes_from_latest_scan_even_when_visible() {
        let mut st = DetectPanelState::new();
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        assert!(st.is_visible());
        // 后续扫描不再返回 sid-a 的进程(已退出)而是找到 sid-b。
        // 批必须反映最新扫描,而非陈旧的快照。
        st.refresh(vec![with_sid("claude-code", 2, Some("sid-b"))]);
        assert_eq!(st.entries.len(), 1);
        assert_eq!(st.entries[0].session_id, "sid-b");
    }

    #[test]
    fn resume_one_removes_entry_and_records_resumed() {
        let mut st = DetectPanelState::new();
        st.refresh(vec![
            with_sid("claude-code", 1, Some("sid-a")),
            with_sid("claude-code", 2, Some("sid-b")),
        ]);
        let r = st.resume_one("sid-a");
        assert_eq!(r, Some(("claude-code".to_string(), "sid-a".to_string())));
        assert_eq!(st.entries.len(), 1);
        assert_eq!(st.entries[0].session_id, "sid-b");
        // sid-a 现已恢复 → 不会再浮现。
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        assert!(!st.is_visible());
    }

    #[test]
    fn resume_one_unknown_session_id_is_none() {
        let mut st = DetectPanelState::new();
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        assert!(st.resume_one("nope").is_none());
        assert_eq!(st.entries.len(), 1);
    }

    #[test]
    fn resume_all_clears_and_records_all() {
        let mut st = DetectPanelState::new();
        st.refresh(vec![
            with_sid("claude-code", 1, Some("sid-a")),
            with_sid("claude-code", 2, Some("sid-b")),
        ]);
        let all = st.resume_all();
        assert_eq!(all.len(), 2);
        assert!(!st.is_visible());
        // 两者均已恢复 → 不会再浮现。
        st.refresh(vec![
            with_sid("claude-code", 1, Some("sid-a")),
            with_sid("claude-code", 2, Some("sid-b")),
        ]);
        assert!(!st.is_visible());
    }

    #[test]
    fn dismiss_all_records_and_clears() {
        let mut st = DetectPanelState::new();
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        st.dismiss_all();
        assert!(!st.is_visible());
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        assert!(!st.is_visible());
    }
}
