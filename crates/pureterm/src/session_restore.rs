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


use crate::split_tree::SplitNode;

mod restore;
mod snapshot;
mod tab;

#[cfg(unix)]
pub use restore::restore;
#[cfg(unix)]
pub use snapshot::save_atomic;

/// 一个持久化的窗格,按与该 tab 的 `split_tree` 对齐的 DFS 叶子顺序排列。
/// `session_id == None` 表示非守护进程(直连 PTY)窗格,它无法
/// 跨 GUI 重启存活;恢复时会将其裁剪掉。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PaneSnapshot {
    pub session_id: Option<String>,
    /// `"local"` 或 `"ssh"`(仅当 `session_id` 为 `Some` 时有效)。
    pub kind: String,
    /// SSH 配置名(用于标题 + 重连元数据查找);本地窗格为 `None`。
    pub ssh_profile: Option<String>,
    pub title: String,
    /// GAP-7.5:agent 会话 id(Claude Code / Codex),用于经
    /// `claude --resume <id>` 冷恢复。普通 shell 为 `None`。`#[serde(default)]`
    /// 保证 v1 快照仍可加载(字段缺失 ⇒ `None`)。
    #[serde(default)]
    pub agent_session_id: Option<String>,
    /// GAP-7.5:agent 类型(`claude-code`/`codex`),与 agent_session_id 成对。
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

/// 磁盘上布局快照的路径。位于守护进程的持久化、
/// 仅所有者(0o700)数据目录下,由 `pureterm_daemon::discovery::ensure_dirs` 创建。
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

/// 加载快照。缺失或无法解析时返回 `None`(视为全新启动)。
/// v2 path reads `workspaces.json`;若无则尝试一次性迁移 v1 `gui-sessions.json`。
pub fn load() -> Option<LayoutSnapshot> {
    // v2 路径:workspaces.json
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

/// 恢复出的窗格:内容载体(终端或 web,GAP-3.5)+ 显示标题。
/// Windows 上无真实 PTY 后端,此结构仅编译不实例化。
pub struct RestoredPane {
    pub content: crate::split_tree::Surface,
    pub title: String,
}

/// 恢复出的 tab:已重映射为全新 `PaneId` 的 split tree,外加
/// 按 DFS 叶子顺序排列的窗格(与 `split_tree.pane_ids()` 对齐)。
/// Windows 上无真实 PTY 后端,此结构仅编译不实例化。
pub struct RestoredTab {
    pub title: String,
    pub split_tree: SplitNode,
    pub panes: Vec<RestoredPane>,
    pub active_pane_index: usize,
}

/// GAP-2.1: 恢复出的一个 workspace(若干 tab + 活跃 tab 索引)。
/// Windows 上无真实 PTY 后端,此结构仅编译不实例化。
pub struct RestoredWorkspace {
    pub name: String,
    pub tabs: Vec<RestoredTab>,
    pub active_tab: usize,
}

/// Windows 上无真实 PTY 后端,此结构仅编译不实例化。
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
