//! GAP-06:环境代理 MVP——本地任务队列 + EventBus 钩子。
//!
//! 云端异步(SSE、webhooks、Slack/GitHub)留待后续。当
//! `[ambient_agents].enabled = true` 时,CLI/MCP 可入队任务;结果
//! 通过通知 + 状态栏 + 可选右侧边栏 Feed 页呈现。

use serde::{Deserialize, Serialize};

/// `[ambient_agents]` 配置。默认关闭——未显式开启前不做后台工作。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmbientAgentsConfig {
    pub enabled: bool,
    /// 本地任务运行器的轮询间隔(秒)。
    pub poll_interval_secs: u64,
    /// 内存中最多保留的已完成任务数。
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
