//! GAP-06: Ambient Agents MVP — local task queue + EventBus hooks.
//!
//! Full cloud async (SSE, webhooks, Slack/GitHub) is deferred. When
//! `[ambient_agents].enabled = true`, CLI/MCP can enqueue tasks; results
//! surface via notifications + status bar + optional right-sidebar Feed tab.

use serde::{Deserialize, Serialize};

/// `[ambient_agents]` config. Default off — no background work until opted in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmbientAgentsConfig {
    pub enabled: bool,
    /// Poll interval for local task runner (seconds).
    pub poll_interval_secs: u64,
    /// Max retained completed tasks in memory.
    pub max_history: usize,
}

impl Default for AmbientAgentsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: 30,
            max_history: 50,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmbientTaskStatus {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmbientTask {
    pub id: u64,
    pub title: String,
    pub source: String,
    pub status: AmbientTaskStatus,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AmbientAgentsState {
    next_id: u64,
    pub tasks: Vec<AmbientTask>,
}

impl AmbientAgentsState {
    pub fn enqueue(&mut self, title: impl Into<String>, source: impl Into<String>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(AmbientTask {
            id,
            title: title.into(),
            source: source.into(),
            status: AmbientTaskStatus::Pending,
            summary: None,
        });
        id
    }

    pub fn pending_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, AmbientTaskStatus::Pending | AmbientTaskStatus::Running))
            .count()
    }

    pub fn mark_done(&mut self, id: u64, summary: impl Into<String>) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.status = AmbientTaskStatus::Done;
            t.summary = Some(summary.into());
        }
    }

    pub fn trim(&mut self, max: usize) {
        if self.tasks.len() > max {
            let drop = self.tasks.len() - max;
            self.tasks.drain(0..drop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_and_complete() {
        let mut st = AmbientAgentsState::default();
        let id = st.enqueue("review PR", "cli");
        assert_eq!(st.pending_count(), 1);
        st.mark_done(id, "ok");
        assert_eq!(st.pending_count(), 0);
    }
}
