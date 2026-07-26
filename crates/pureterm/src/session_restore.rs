//! 启动时从持久化快照恢复 tab/pane/SSH/agent 状态。
//!
//! 快照格式:`~/.local/share/pureterm/workspaces.json`(V2),V1 旧格式
//! `gui-sessions.json` 自动迁移一次。
//!
//! 模块布局:
//! - 根:类型定义(Pane/Tab/Viewport/Workspace/Window/Layout Snapshot +
//!   Restored*)+ 常量 + state_path + legacy_v1_path + V1→V2 变换(load/migrate/wrap_v1)
//! - `snapshot`:V1→V2 迁移核心 `migrate_v1_to_v2_in` + save_atomic_to + save_atomic
//! - `restore`:`restore`(整体 LayoutSnapshot → GUI)+ `cold_resume_agent`
//! - `tab`:`restore_one_tab`(单个 tab → 创建 pane/pty/ssh)

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::split_tree::SplitNode;

mod restore;
mod snapshot;
mod tab;

pub use restore::{cold_resume_agent, restore};
pub use snapshot::{save_atomic, save_atomic_to};

/// One persisted pane, in DFS leaf order aligned with the tab's `split_tree`.
/// `session_id == None` marks a non-daemon (direct PTY) pane, which cannot
/// survive GUI restart; restore prunes it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PaneSnapshot {
    pub session_id: Option<String>,
    /// `"local"` or `"ssh"` (only meaningful when `session_id` is `Some`).
    pub kind: String,
    /// SSH profile name (for title + reconnect metadata lookup); `None` for local.
    pub ssh_profile: Option<String>,
    pub title: String,
    /// GAP-7.5: agent session id (Claude Code / Codex) for cold resume via
    /// `claude --resume <id>`. `None` for plain shells. `#[serde(default)]`
    /// keeps v1 snapshots loadable (field absent ⇒ `None`).
    #[serde(default)]
    pub agent_session_id: Option<String>,
    /// GAP-7.5: agent kind (`claude-code`/`codex`), pairs with agent_session_id.
    #[serde(default)]
    pub agent_kind: Option<String>,
    /// GAP-3.5: web pane 的 URL(仅 `kind == "web"` 时有值)。`#[serde(default)]`
    /// 让 v1 快照可加载(web pane 是后加字段)。terminal pane 为 `None`。
    /// 不 bump `SNAPSHOT_VERSION`:仅新增可选字段,serde 默认值即前向兼容,
    /// bump 反而会让现存 v1 快照被 `load()` 拒绝(丢会话)。
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TabSnapshot {
    pub title: String,
    pub active_pane_leaf_index: usize,
    pub split_tree: SplitNode,
    pub panes: Vec<PaneSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ViewportSnapshot {
    pub w: f32,
    pub h: f32,
}

/// GAP-2.1: 一个命名 workspace 的快照(若干 tab + 活跃 tab 索引)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceSnapshot {
    pub name: String,
    pub active_tab: usize,
    pub tabs: Vec<TabSnapshot>,
}

/// GAP-2.1: 一个顶级窗口的快照。主窗 `is_main=true`;detached `is_main=false`。
/// 恢复时所有窗口的 workspace 折叠进主窗(见 `restore`),故窗口拓扑不跨重启,
/// 但 PTY/session/scrollback 全保留。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WindowSnapshot {
    pub is_main: bool,
    pub active_workspace: usize,
    pub workspaces: Vec<WorkspaceSnapshot>,
}

/// v2:窗口/workspace/tab 完整树(替换 v1 扁平 `{ active_tab, tabs }`)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayoutSnapshot {
    pub version: u32,
    pub viewport: ViewportSnapshot,
    pub windows: Vec<WindowSnapshot>,
}

pub const SNAPSHOT_VERSION: u32 = 2;
const SNAPSHOT_FILE: &str = "workspaces.json";
const LEGACY_V1_FILE: &str = "gui-sessions.json";

/// Path of the on-disk layout snapshot. Lives in the daemon's persistent,
/// owner-only (0o700) data dir, created by `pureterm_daemon::discovery::ensure_dirs`.
pub fn state_path() -> PathBuf {
    pureterm_daemon::discovery::data_dir().join(SNAPSHOT_FILE)
}

/// v1 遗留快照路径(`gui-sessions.json`)。
fn legacy_v1_path() -> PathBuf {
    pureterm_daemon::discovery::data_dir().join(LEGACY_V1_FILE)
}

/// v1 遗留快照(扁平 `{ active_tab, tabs }`),仅用于迁移解析。
#[derive(Debug, serde::Deserialize)]
struct LegacyV1Snapshot {
    version: u32,
    viewport: ViewportSnapshot,
    active_tab: usize,
    tabs: Vec<TabSnapshot>,
}

/// GAP-2.1: 把 v1 扁平快照包进 v2 树(单主窗 + 单 "Default" workspace)。
/// 纯函数 —— 迁移变换可 headless 单测(文件搬运另见 `migrate_v1_to_v2`)。
pub fn wrap_v1_into_v2(
    tabs: Vec<TabSnapshot>,
    active_tab: usize,
    viewport: ViewportSnapshot,
) -> LayoutSnapshot {
    LayoutSnapshot {
        version: SNAPSHOT_VERSION,
        viewport,
        windows: vec![WindowSnapshot {
            is_main: true,
            active_workspace: 0,
            workspaces: vec![WorkspaceSnapshot {
                name: "Default".to_string(),
                active_tab,
                tabs,
            }],
        }],
    }
}

/// Load the snapshot. `None` if missing or unparseable (treated as fresh start).
/// v2 path reads `workspaces.json`;若无则尝试一次性迁移 v1 `gui-sessions.json`。
pub fn load() -> Option<LayoutSnapshot> {
    // v2 path: workspaces.json
    if let Ok(data) = std::fs::read(state_path()) {
        match serde_json::from_slice::<LayoutSnapshot>(&data) {
            Ok(s) if s.version == SNAPSHOT_VERSION => return Some(s),
            Ok(other) => log::warn!(
                "session_restore: ignoring v2 snapshot version {} (expected {})",
                other.version,
                SNAPSHOT_VERSION
            ),
            Err(e) => log::warn!("session_restore: v2 parse failed: {e}"),
        }
    }
    // v1 migration path: legacy gui-sessions.json → v2(一次性,.bak 备份)。
    migrate_v1_to_v2()
}

/// 读 v1 `gui-sessions.json` → 包成 v2 → 持久化 v2(成功后旧文件改名 `.bak`,
/// 永不 unlink)→ 下次 `load` 直接命中 v2 路径。任一步失败均回退 fresh start(None)。
fn migrate_v1_to_v2() -> Option<LayoutSnapshot> {
    #[cfg(unix)]
    {
        snapshot::migrate_v1_to_v2_in(&legacy_v1_path(), &state_path())
    }
    #[cfg(not(unix))]
    {
        None
    }
}

// ── 恢复结果类型(Unix only — 恢复涉及 spawn PTY/SSH) ──────────────────────

/// A restored pane: its content carrier (terminal or web, GAP-3.5) + display title.
#[cfg(unix)]
pub struct RestoredPane {
    pub content: crate::split_tree::Surface,
    pub title: String,
}

/// A restored tab: a split tree already remapped to fresh `PaneId`s, plus its
/// panes in DFS leaf order (aligned with `split_tree.pane_ids()`).
#[cfg(unix)]
pub struct RestoredTab {
    pub title: String,
    pub split_tree: SplitNode,
    pub panes: Vec<RestoredPane>,
    pub active_pane_index: usize,
}

/// GAP-2.1: 恢复出的一个 workspace(若干 tab + 活跃 tab 索引)。
#[cfg(unix)]
pub struct RestoredWorkspace {
    pub name: String,
    pub tabs: Vec<RestoredTab>,
    pub active_tab: usize,
}

#[cfg(unix)]
pub struct RestoreOutcome {
    /// 折叠进主窗的 workspace 列表(主窗自身的在前,detached 的追加在后;
    /// 孤儿 daemon 会话进末尾 "Recovered" workspace)。窗口拓扑不跨重启(折叠),
    /// 但 PTY/session/scrollback 全保留。
    pub workspaces: Vec<RestoredWorkspace>,
    pub active_workspace: usize,
}

#[cfg(test)]
#[path = "session_restore/tests.rs"]
mod tests;