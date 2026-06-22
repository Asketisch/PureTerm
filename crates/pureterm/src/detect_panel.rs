//! GAP-7.6: detect-resume 提示面板 —— GUI 侧状态机。
//!
//! When `DetectScanTick` (iced_app) runs `agent_detect::scan` and finds running
//! agent processes whose kind has a persisted `session_id`, the results are
//! pushed here. The right-side pinned overlay shows them; the user resumes one
//! or all (→ `create_resume_tab` spawns `claude --resume <id>` in a new pane)
//! or dismisses. Already-dismissed / already-resumed session_ids are suppressed
//! for the process lifetime (detection is a *hint*, not a critical path, so
//! re-prompting once after a restart is acceptable).
//!
//! Mirrors `hibernate_panel::HibernationPanelState`'s shape, but holds a batch
//! (`Vec`) rather than a single confirmation, and tracks dismissed/resumed ids.

use std::collections::HashSet;

use crate::agent_detect::DetectedAgent;

/// One resumable detected agent (None session_id already filtered out, deduped).
#[derive(Debug, Clone)]
pub struct DetectedResumeEntry {
    /// Matching `[[vault.agents]]` kind (e.g. "claude-code").
    pub kind: String,
    /// Persisted agent session id to pass to the resume command.
    pub session_id: String,
    /// Host pid of the matched process (display only).
    pub pid: u32,
}

/// Detect-resume panel state. Always present on the app; visibility =
/// `!entries.is_empty()`.
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

    /// Whether the overlay panel should render.
    pub fn is_visible(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Refresh from a scan result (`agent_detect::with_sessions` output): drop
    /// entries with no session_id, skip already dismissed/resumed, dedup by
    /// session_id. `entries` always reflects the *latest* scan (the truth is
    /// "currently-running, non-dismissed/resumed agents with a session_id"), so
    /// a process that exits drops out and a newly-spawned one appears. Scan is
    /// deterministic and runs every ~30s, so flapping risk is negligible, and
    /// resume/dismiss filtering takes effect on the next tick.
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

    /// User picked one entry → returns `(kind, session_id)`, records it resumed,
    /// removes it from the batch.
    pub fn resume_one(&mut self, session_id: &str) -> Option<(String, String)> {
        let pos = self
            .entries
            .iter()
            .position(|e| e.session_id == session_id)?;
        let e = self.entries.remove(pos);
        self.resumed.insert(e.session_id.clone());
        Some((e.kind, e.session_id))
    }

    /// User picked "resume all" → returns every `(kind, session_id)`, records
    /// all resumed, clears the batch.
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

    /// User dismissed the batch ("稍后" / ×) → records all current session_ids
    /// as dismissed, clears the batch.
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
        // Two host pids cross-referenced to the same kind's latest session_id.
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
        // Re-scan: sid-d was dismissed → skipped; sid-e still surfaces.
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
        // Re-scan: sid-a was resumed → skipped.
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        assert!(!st.is_visible());
    }

    #[test]
    fn refresh_recomputes_from_latest_scan_even_when_visible() {
        let mut st = DetectPanelState::new();
        st.refresh(vec![with_sid("claude-code", 1, Some("sid-a"))]);
        assert!(st.is_visible());
        // A later scan no longer returns sid-a's process (it exited) but finds
        // sid-b. The batch must reflect the latest scan, not a stale snapshot.
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
        // sid-a now resumed → won't re-surface.
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
        // Both now resumed → won't re-surface.
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
