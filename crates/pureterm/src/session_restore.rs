//! GAP-5.6 cross-restart session recovery: GUI-side persistence of the tab/
//! pane layout + restoration that re-attaches to daemon sessions still alive
//! after a GUI restart.
//!
//! The daemon (`puretermd`) keeps PTY sessions alive across GUI restart; on
//! re-launch the GUI reads this snapshot, checks `session.list` for survivors,
//! and re-attaches. Scrollback is replayed by the daemon as `SCROLLBACK`
//! frames, so each freshly-built `Term` + `vte::ansi::Processor` rebuilds its
//! grid + scrollback naturally.
//!
//! Persistence is debounced (caller) + atomic (`NamedTempFile::persist`); a
//! crash still leaves the last flushed snapshot on disk.

use std::path::PathBuf;

use crate::split_tree::SplitNode;

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
/// v1 遗留文件名(迁移源;迁移后重命名 `.bak`,永不 unlink)。
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

/// 读 v1 `gui-sessions.json` → 包成 v2 → 旧文件重命名 `.bak`(永不 unlink)→
/// 持久化 v2(下次 `load` 直接命中 v2 路径)。任一步失败均回退 fresh start(None)。
fn migrate_v1_to_v2() -> Option<LayoutSnapshot> {
    let legacy_path = legacy_v1_path();
    let data = std::fs::read(&legacy_path).ok()?;
    let v1 = match serde_json::from_slice::<LegacyV1Snapshot>(&data) {
        Ok(s) if s.version == 1 => s,
        Ok(other) => {
            log::warn!(
                "session_restore: legacy version {} (expected 1), skipping",
                other.version
            );
            return None;
        }
        Err(e) => {
            log::warn!("session_restore: legacy v1 parse failed: {e}");
            return None;
        }
    };
    let v2 = wrap_v1_into_v2(v1.tabs, v1.active_tab, v1.viewport);
    // 旧文件改名 .bak(保留,永不 unlink,便于回退/排查)。
    let bak = legacy_path.with_extension("json.bak");
    if let Err(e) = std::fs::rename(&legacy_path, &bak) {
        log::warn!("session_restore: rename legacy → .bak failed: {e}");
    }
    // 持久化 v2,下次启动直接命中 v2 路径(best-effort,失败不阻塞)。
    #[cfg(unix)]
    save_atomic(&v2);
    log::info!("session_restore: migrated v1 gui-sessions.json → v2 workspaces.json");
    Some(v2)
}

/// Atomically write the snapshot. Best-effort: logs on failure (recovery is
/// optional — a failed write must never crash the GUI).
#[cfg(unix)]
pub fn save_atomic(snapshot: &LayoutSnapshot) {
    use std::io::Write;

    let path = state_path();
    let Some(dir) = path.parent() else {
        log::warn!("session_restore: snapshot path has no parent");
        return;
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("session_restore: create_dir_all failed: {e}");
        return;
    }
    let json = match serde_json::to_string_pretty(snapshot) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("session_restore: serialize failed: {e}");
            return;
        }
    };
    let mut tmp = match tempfile::NamedTempFile::new_in(dir) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("session_restore: temp file failed: {e}");
            return;
        }
    };
    if let Err(e) = tmp.write_all(json.as_bytes()) {
        log::warn!("session_restore: write failed: {e}");
        return;
    }
    if let Err(e) = tmp.persist(&path) {
        log::warn!("session_restore: persist failed: {e}");
    }
}

#[cfg(unix)]
use std::sync::Arc;

#[cfg(unix)]
use crate::split_tree::PaneId;
#[cfg(unix)]
use crate::ssh::config::SshProfile;
#[cfg(unix)]
use crate::terminal_context::TerminalContext;

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

/// GAP-2.1: 恢复单个 tab —— 重连存活 daemon 会话 / 剪死 pane / agent 冷恢复 /
/// PaneId 重映射。整 tab 死亡返回 `None`。从 `restore()` 抽出,让 window→workspace
/// 嵌套循环复用同一份逻辑(此前 v1 扁平循环的内联体)。
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn restore_one_tab(
    tab_snap: &TabSnapshot,
    alive_ids: &std::collections::HashSet<&str>,
    consumed: &mut std::collections::HashSet<String>,
    ssh_profiles: &[SshProfile],
) -> Option<RestoredTab> {
    let leaf_ids = tab_snap.split_tree.pane_ids();
    if tab_snap.panes.len() != leaf_ids.len() {
        log::warn!(
            "session_restore: tab pane/leaf count mismatch ({} != {}); skipping tab",
            tab_snap.panes.len(),
            leaf_ids.len()
        );
        return None;
    }

    let mut tree = tab_snap.split_tree.clone();
    let mut alive_panes: Vec<(PaneId, RestoredPane)> = Vec::new();
    let active_old_id = leaf_ids.get(tab_snap.active_pane_leaf_index).copied();
    let mut active_survived_old_id: Option<PaneId> = None;

    for (i, old_id) in leaf_ids.iter().enumerate() {
        let pane_snap = &tab_snap.panes[i];

        // GAP-3.5: web pane — 无 daemon session,不参与存活/重连判定,
        // 直接按快照里的 url 重建一个 BrowserContext。
        if pane_snap.kind == "web" {
            let url = pane_snap.url.clone().unwrap_or_else(|| "about:blank".to_string());
            let browser = crate::browser::BrowserContext::new(url);
            if Some(*old_id) == active_old_id {
                active_survived_old_id = Some(*old_id);
            }
            alive_panes.push((
                *old_id,
                RestoredPane {
                    content: crate::split_tree::Surface::Web(browser),
                    title: pane_snap.title.clone(),
                },
            ));
            continue;
        }

        let Some(sid) = pane_snap.session_id.as_ref() else {
            // Non-daemon pane (direct PTY) — cannot survive. Prune.
            let _ = tree.remove(*old_id);
            continue;
        };
        if !alive_ids.contains(sid.as_str()) {
            // GAP-7.5: session died while the GUI was closed. If this pane
            // tracked an agent session id, cold-resume it (e.g.
            // `claude --resume <id>`) into a fresh daemon session; otherwise
            // prune (a plain shell can't be revived).
            if let Some(agent_id) = pane_snap.agent_session_id.as_ref() {
                match cold_resume_agent(agent_id, pane_snap.agent_kind.as_deref()) {
                    Ok(ctx) => {
                        if Some(*old_id) == active_old_id {
                            active_survived_old_id = Some(*old_id);
                        }
                        alive_panes.push((
                            *old_id,
                            RestoredPane {
                                content: crate::split_tree::Surface::Terminal(ctx),
                                title: pane_snap.title.clone(),
                            },
                        ));
                        continue;
                    }
                    Err(e) => log::warn!(
                        "session_restore: cold-resume agent {} failed ({e}); pruning",
                        agent_id
                    ),
                }
            }
            // Plain shell (no agent_session_id), or cold-resume failed: prune.
            let _ = tree.remove(*old_id);
            continue;
        }
        match attach_pane(sid, &pane_snap.kind, pane_snap.ssh_profile.as_deref(), ssh_profiles) {
            Ok(ctx) => {
                consumed.insert(sid.clone());
                if Some(*old_id) == active_old_id {
                    active_survived_old_id = Some(*old_id);
                }
                alive_panes.push((
                    *old_id,
                    RestoredPane {
                        content: crate::split_tree::Surface::Terminal(ctx),
                        title: pane_snap.title.clone(),
                    },
                ));
            }
            Err(e) => {
                log::warn!("session_restore: attach {} failed ({e}); pruning", sid);
                let _ = tree.remove(*old_id);
            }
        }
    }

    if alive_panes.is_empty() {
        return None; // whole tab died
    }

    // Remap surviving old PaneIds -> fresh ones, walking the pruned tree's
    // new leaf order so panes line up with the rebuilt split tree.
    let new_leaf_order = tree.pane_ids();
    debug_assert_eq!(new_leaf_order.len(), alive_panes.len());
    let mut mapping: std::collections::HashMap<PaneId, PaneId> =
        std::collections::HashMap::new();
    let mut panes_in_order: Vec<RestoredPane> = Vec::with_capacity(alive_panes.len());
    let mut active_index = 0usize;
    for (i, old_id) in new_leaf_order.iter().enumerate() {
        let fresh = PaneId::next();
        mapping.insert(*old_id, fresh);
        if let Some(pos) = alive_panes.iter().position(|(id, _)| id == old_id) {
            panes_in_order.push(alive_panes.swap_remove(pos).1);
            if active_survived_old_id == Some(*old_id) {
                active_index = i;
            }
        }
    }
    tree.remap_pane_ids(&mapping);
    tree.regenerate_split_ids();

    Some(RestoredTab {
        title: tab_snap.title.clone(),
        split_tree: tree,
        panes: panes_in_order,
        active_pane_index: active_index,
    })
}

/// Attempt to restore the previous layout by re-attaching to surviving daemon
/// sessions. Returns `None` if nothing could be restored (caller opens a fresh
/// tab). GAP-2.1: walks the v2 window→workspace→tab tree; all windows' workspaces
/// fold into the main window (detached topology doesn't survive restart, but every
/// PTY/session/scrollback does). Orphans (alive daemon sessions not in the
/// snapshot) land in a trailing "Recovered" workspace. Panes whose session died
/// (or were non-daemon) are pruned and the split tree collapses around them.
#[cfg(unix)]
pub fn restore(
    snapshot: &LayoutSnapshot,
    ssh_profiles: &[SshProfile],
) -> Option<RestoreOutcome> {
    use crate::daemon_client;

    let daemon_socket = match daemon_client::ensure_daemon() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("session_restore: daemon unavailable ({e}); skipping recovery");
            return None;
        }
    };

    // Alive sessions: id -> kind (kind is needed for orphan dispatch).
    let alive: Vec<(String, String)> = match daemon_client::list_sessions(&daemon_socket) {
        Ok(list) => list
            .into_iter()
            .filter_map(|v| {
                let id = v.get("id")?.as_str()?.to_string();
                let kind = v.get("kind")?.as_str()?.to_string();
                Some((id, kind))
            })
            .collect(),
        Err(e) => {
            log::warn!("session_restore: session.list failed ({e}); skipping recovery");
            return None;
        }
    };
    let alive_ids: std::collections::HashSet<&str> =
        alive.iter().map(|(id, _)| id.as_str()).collect();
    let mut consumed: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 主窗先处理(其 workspace 落在折叠列表前部,active_workspace 索引稳定)。
    let mut win_order: Vec<&WindowSnapshot> = snapshot.windows.iter().collect();
    win_order.sort_by_key(|w| !w.is_main);

    let mut restored_workspaces: Vec<RestoredWorkspace> = Vec::new();
    let mut active_workspace: usize = 0;

    for win_snap in win_order {
        let is_main = win_snap.is_main;
        for (ws_idx, ws_snap) in win_snap.workspaces.iter().enumerate() {
            let mut tabs: Vec<RestoredTab> = Vec::new();
            let mut active_tab: usize = 0;
            for (tab_idx, tab_snap) in ws_snap.tabs.iter().enumerate() {
                if let Some(rtab) =
                    restore_one_tab(tab_snap, &alive_ids, &mut consumed, ssh_profiles)
                {
                    if tab_idx == ws_snap.active_tab {
                        active_tab = tabs.len();
                    }
                    tabs.push(rtab);
                }
            }
            if tabs.is_empty() {
                continue; // 整个 workspace 的 tab 全死
            }
            restored_workspaces.push(RestoredWorkspace {
                name: ws_snap.name.clone(),
                tabs,
                active_tab,
            });
            if is_main && ws_idx == win_snap.active_workspace {
                active_workspace = restored_workspaces.len() - 1;
            }
        }
    }

    // Orphans: alive sessions not referenced by any snapshot pane. 落进末尾
    // "Recovered" workspace,daemon 持有的会话不静默丢失。
    let mut orphan_tabs: Vec<RestoredTab> = Vec::new();
    for (sid, kind) in &alive {
        if consumed.contains(sid) {
            continue;
        }
        match attach_pane(sid, kind, None, ssh_profiles) {
            Ok(ctx) => {
                let title = if kind == "ssh" {
                    "🔒 SSH".to_string()
                } else {
                    "会话".to_string()
                };
                let fresh = PaneId::next();
                orphan_tabs.push(RestoredTab {
                    title: title.clone(),
                    split_tree: SplitNode::single(fresh),
                    panes: vec![RestoredPane {
                        content: crate::split_tree::Surface::Terminal(ctx),
                        title,
                    }],
                    active_pane_index: 0,
                });
            }
            Err(e) => log::warn!("session_restore: orphan attach {} failed: {e}", sid),
        }
    }
    if !orphan_tabs.is_empty() {
        restored_workspaces.push(RestoredWorkspace {
            name: "Recovered".to_string(),
            tabs: orphan_tabs,
            active_tab: 0,
        });
    }

    if restored_workspaces.is_empty() {
        None
    } else {
        if active_workspace >= restored_workspaces.len() {
            active_workspace = 0;
        }
        Some(RestoreOutcome {
            workspaces: restored_workspaces,
            active_workspace,
        })
    }
}

/// GAP-7.5: cold-resume an agent whose daemon session died while the GUI was
/// closed. Resolves the agent's `resume_command` from `[[vault.agents]]` (by
/// kind), substitutes `{session_id}`, spawns it via the daemon (which owns the
/// new PTY), and attaches. Falls back to `claude --resume <id>` when the kind
/// isn't configured. Built at 80×24 — the caller resizes after attach.
#[cfg(unix)]
fn cold_resume_agent(
    agent_session_id: &str,
    agent_kind: Option<&str>,
) -> std::io::Result<Arc<TerminalContext>> {
    use alacritty_terminal::term::test::TermSize;

    let kind = agent_kind.unwrap_or("claude-code");
    // GAP-7.7: prefer an `auto` resume binding (e.g. `tmux attach -t work`)
    // authorized by an HMAC token over the default `claude --resume`. Cold-
    // resume never prompts — no token (or no auto binding) means fall through
    // to the vault/claude path below.
    if let Some(argv) = crate::resume_bindings::cold_resume_argv(kind, agent_session_id, "") {
        let daemon_socket = crate::daemon_client::ensure_daemon()?;
        let (sid, _data_path) = crate::daemon_client::create_local(
            &daemon_socket,
            80,
            24,
            None,
            Some(&argv),
            Some(kind),
        )?;
        let ctx = TerminalContext::attach_local_via_daemon(&sid, TermSize::new(80, 24))?;
        return Ok(Arc::new(ctx));
    }
    let argv = crate::app_config::AppConfig::load_vault_agents(
        &crate::app_config::AppConfig::config_path(),
    )
    .into_iter()
    .find(|e| e.kind == kind)
    .map(|e| e.resume_command.clone())
    .unwrap_or_else(|| {
        // Default: `claude --resume <id>` (the most common agent).
        vec!["claude".into(), "--resume".into(), agent_session_id.to_string()]
    });
    // Substitute the `{session_id}` placeholder in each argv element.
    let argv: Vec<String> =
        argv.iter().map(|a| a.replace("{session_id}", agent_session_id)).collect();

    let daemon_socket = crate::daemon_client::ensure_daemon()?;
    let (sid, _data_path) = crate::daemon_client::create_local(
        &daemon_socket,
        80,
        24,
        None,
        Some(&argv),
        Some(kind),
    )?;
    let ctx = TerminalContext::attach_local_via_daemon(&sid, TermSize::new(80, 24))?;
    Ok(Arc::new(ctx))
}

/// Attach one pane's terminal context to a surviving daemon session. Local and
/// SSH both go through `attach_existing` → `DaemonPty`; SSH reuses the live
/// `ssh -tt` child (no re-auth). Built at a placeholder 80×24 — the caller
/// resizes to the real window right after attach.
#[cfg(unix)]
fn attach_pane(
    session_id: &str,
    kind: &str,
    ssh_profile_name: Option<&str>,
    ssh_profiles: &[SshProfile],
) -> std::io::Result<Arc<TerminalContext>> {
    use alacritty_terminal::term::test::TermSize;
    let size = TermSize::new(80, 24);
    if kind == "ssh" {
        let profile = ssh_profile_name
            .and_then(|name| ssh_profiles.iter().find(|p| p.name == name))
            .cloned()
            .unwrap_or_else(|| synthetic_ssh_profile(ssh_profile_name));
        let ctx = TerminalContext::attach_ssh_via_daemon(session_id, &profile, size)?;
        Ok(Arc::new(ctx))
    } else {
        let ctx = TerminalContext::attach_local_via_daemon(session_id, size)?;
        Ok(Arc::new(ctx))
    }
}

/// A minimal placeholder profile for an SSH session whose original profile was
/// deleted from config (or was never persisted). The remote shell keeps
/// running via the daemon; only the display name + reconnect metadata are
/// best-effort.
#[cfg(unix)]
fn synthetic_ssh_profile(name: Option<&str>) -> SshProfile {
    SshProfile {
        name: name.unwrap_or("SSH").to_string(),
        host: String::new(),
        user: None,
        port: None,
        password: None,
        key_path: None,
        shortcut: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个单 pane 的 TabSnapshot(减少 v2 测试样板的本地 helper)。
    fn one_pane_tab_snap(title: &str, pid: u64, pane: PaneSnapshot) -> TabSnapshot {
        TabSnapshot {
            title: title.to_string(),
            active_pane_leaf_index: 0,
            split_tree: SplitNode::single(crate::split_tree::PaneId(pid)),
            panes: vec![pane],
        }
    }

    /// 把 tab 列表包进单主窗单 "Default" workspace(v2 树最小壳)。
    fn wrap_tabs_into_v2(tabs: Vec<TabSnapshot>, active_tab: usize) -> LayoutSnapshot {
        LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 1024.0, h: 768.0 },
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

    /// Serialize → deserialize must preserve the session id and the split tree
    /// (the on-disk contract for cross-restart recovery). GAP-2.1: v2 tree shape.
    #[test]
    fn snapshot_round_trip() {
        let snap = wrap_tabs_into_v2(
            vec![one_pane_tab_snap(
                "zsh",
                1,
                PaneSnapshot {
                    session_id: Some("daemon-1".to_string()),
                    kind: "local".to_string(),
                    ssh_profile: None,
                    title: "zsh".to_string(),
                    agent_session_id: Some("cc-abc".to_string()),
                    agent_kind: Some("claude-code".to_string()),
                    url: None,
                },
            )],
            0,
        );
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: LayoutSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.version, SNAPSHOT_VERSION);
        assert_eq!(back.windows.len(), 1);
        assert!(back.windows[0].is_main);
        let panes = &back.windows[0].workspaces[0].tabs[0].panes;
        assert_eq!(panes[0].session_id.as_deref(), Some("daemon-1"));
        // GAP-7.5: agent fields survive the round-trip (cold-resume persistence).
        assert_eq!(panes[0].agent_session_id.as_deref(), Some("cc-abc"));
        assert_eq!(panes[0].agent_kind.as_deref(), Some("claude-code"));
        assert_eq!(
            back.windows[0].workspaces[0].tabs[0].split_tree.pane_ids(),
            vec![crate::split_tree::PaneId(1)]
        );
    }

    /// GAP-3.5: a web pane (kind="web", session_id=None, url=Some) must survive
    /// the round-trip so it can be rebuilt by `restore()` from its url.
    #[test]
    fn web_pane_snapshot_round_trip() {
        let snap = wrap_tabs_into_v2(
            vec![one_pane_tab_snap(
                "🌐 Browser",
                7,
                PaneSnapshot {
                    session_id: None,
                    kind: "web".to_string(),
                    ssh_profile: None,
                    title: "🌐 Browser".to_string(),
                    agent_session_id: None,
                    agent_kind: None,
                    url: Some("https://example.com".to_string()),
                },
            )],
            0,
        );
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: LayoutSnapshot = serde_json::from_str(&json).expect("deserialize");
        let p = &back.windows[0].workspaces[0].tabs[0].panes[0];
        assert_eq!(p.kind, "web");
        assert!(p.session_id.is_none());
        assert_eq!(p.url.as_deref(), Some("https://example.com"));
    }

    /// GAP-7.5 / GAP-3.5: a pre-existing (v1) pane has no agent_session_id /
    /// agent_kind / url. `#[serde(default)]` must fill them with `None`.
    #[test]
    fn pane_snapshot_v1_without_optional_fields_loads_with_defaults() {
        let json = r#"{"session_id":"s","kind":"local","ssh_profile":null,"title":"t"}"#;
        let p: PaneSnapshot = serde_json::from_str(json).expect("deserialize v1 pane");
        assert_eq!(p.session_id.as_deref(), Some("s"));
        assert!(p.agent_session_id.is_none());
        assert!(p.agent_kind.is_none());
        assert!(p.url.is_none());
    }

    /// A snapshot with the wrong version is rejected by `load`'s version gate.
    /// (We re-implement the same check here since `load` reads a fixed path.)
    #[test]
    fn wrong_version_round_trip_is_distinguishable() {
        let mut snap = LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 10.0, h: 10.0 },
            windows: vec![],
        };
        let _ok = serde_json::to_string(&snap).unwrap();
        snap.version = SNAPSHOT_VERSION + 1;
        let json = serde_json::to_string(&snap).unwrap();
        let back: LayoutSnapshot = serde_json::from_str(&json).unwrap();
        assert_ne!(back.version, SNAPSHOT_VERSION);
    }

    /// GAP-2.1: `wrap_v1_into_v2` 把 v1 扁平 tabs 包成单主窗 + 单 "Default"
    /// workspace,active_tab 透传 —— v1→v2 迁移的数据契约(纯函数,可 headless 锁)。
    #[test]
    fn wrap_v1_into_v2_wraps_flat_tabs_into_one_main_workspace() {
        let pane_local = |sid: &str| PaneSnapshot {
            session_id: Some(sid.into()),
            kind: "local".into(),
            ssh_profile: None,
            title: sid.into(),
            agent_session_id: None,
            agent_kind: None,
            url: None,
        };
        let tabs = vec![
            one_pane_tab_snap("a", 1, pane_local("s1")),
            one_pane_tab_snap("b", 2, pane_local("s2")),
        ];
        let v2 = wrap_v1_into_v2(tabs, 1, ViewportSnapshot { w: 100.0, h: 100.0 });
        assert_eq!(v2.version, SNAPSHOT_VERSION);
        assert_eq!(v2.viewport.w, 100.0);
        assert_eq!(v2.windows.len(), 1, "v1 折成单窗");
        assert!(v2.windows[0].is_main, "主窗");
        assert_eq!(v2.windows[0].workspaces.len(), 1, "单 Default workspace");
        let ws = &v2.windows[0].workspaces[0];
        assert_eq!(ws.name, "Default");
        assert_eq!(ws.tabs.len(), 2, "两个 tab 原样保留");
        assert_eq!(ws.active_tab, 1, "active_tab 透传");
        // 再 round-trip 仍稳定(v2 自洽)。
        let json = serde_json::to_string(&v2).unwrap();
        let back: LayoutSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.windows[0].workspaces[0].tabs.len(), 2);
    }

    /// GAP-2.1: v2 多窗口快照(主窗 + detached,各含 workspace)round-trip 保结构。
    /// 恢复时折叠进主窗,但快照本身忠实记录窗口/workspace 拓扑。
    #[test]
    fn v2_multi_window_snapshot_round_trip() {
        let pane_local = |sid: &str| PaneSnapshot {
            session_id: Some(sid.into()),
            kind: "local".into(),
            ssh_profile: None,
            title: sid.into(),
            agent_session_id: None,
            agent_kind: None,
            url: None,
        };
        let snap = LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 1024.0, h: 768.0 },
            windows: vec![
                WindowSnapshot {
                    is_main: true,
                    active_workspace: 0,
                    workspaces: vec![
                        WorkspaceSnapshot {
                            name: "Default".into(),
                            active_tab: 0,
                            tabs: vec![one_pane_tab_snap("main-t", 1, pane_local("m1"))],
                        },
                        WorkspaceSnapshot {
                            name: "Work".into(),
                            active_tab: 0,
                            tabs: vec![one_pane_tab_snap("work-t", 2, pane_local("m2"))],
                        },
                    ],
                },
                WindowSnapshot {
                    is_main: false,
                    active_workspace: 0,
                    workspaces: vec![WorkspaceSnapshot {
                        name: "Detached".into(),
                        active_tab: 0,
                        tabs: vec![one_pane_tab_snap("det-t", 3, pane_local("d1"))],
                    }],
                },
            ],
        };
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: LayoutSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.windows.len(), 2);
        // 主窗 2 workspace,detached 1 workspace —— 拓扑忠实保留在快照里。
        let main = back.windows.iter().find(|w| w.is_main).unwrap();
        assert_eq!(main.workspaces.len(), 2);
        assert_eq!(main.workspaces[1].name, "Work");
        let det = back.windows.iter().find(|w| !w.is_main).unwrap();
        assert_eq!(det.workspaces.len(), 1);
        assert_eq!(det.workspaces[0].tabs[0].panes[0].session_id.as_deref(), Some("d1"));
    }
}
