//! 整体恢复 —— `restore`(LayoutSnapshot → GUI 状态)+ `cold_resume_agent`(冷启动恢复 agent)。

use std::sync::Arc;

use super::{
    LayoutSnapshot, RestoredPane, RestoredTab, RestoredWorkspace, RestoreOutcome, TabSnapshot,
    WindowSnapshot, WorkspaceSnapshot,
};
use super::tab::restore_one_tab;
use crate::ssh::config::SshProfile;
use crate::split_tree::{PaneId, SplitNode};
use crate::terminal_context::TerminalContext;

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
pub fn cold_resume_agent(
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
            true,
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
        true,
    )?;
    let ctx = TerminalContext::attach_local_via_daemon(&sid, TermSize::new(80, 24))?;
    Ok(Arc::new(ctx))
}

/// Attach one pane's terminal context to a surviving daemon session. Local and
/// SSH both go through `attach_existing` → `DaemonPty`; SSH reuses the live
/// `ssh -tt` child (no re-auth). Built at a placeholder 80×24 — the caller
/// resizes to the real window right after attach.
#[cfg(unix)]
pub(super) fn attach_pane(
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
        ..Default::default()
    }
}
