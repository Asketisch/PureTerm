//! 整体恢复 —— `restore`(LayoutSnapshot → GUI 状态)+ `cold_resume_agent`(冷启动恢复 agent)。

use std::sync::Arc;

use super::{
    LayoutSnapshot, RestoredPane, RestoredTab, RestoredWorkspace, RestoreOutcome,
    WindowSnapshot,
};
#[cfg(unix)]
use super::tab::restore_one_tab;
use crate::ssh::config::SshProfile;
use crate::split_tree::{PaneId, SplitNode};
use crate::terminal_context::TerminalContext;

/// 尝试通过重新附加到仍存活的守护进程会话,恢复上次的布局。
/// 若无可恢复内容则返回 `None`(调用方打开新
/// 标签页)。GAP-2.1:遍历 v2 的 窗口→工作区→标签页 树;所有窗口的
/// 工作区折叠进主窗口(分离式拓扑不跨重启,但每个
/// PTY / 会话 / 回滚都保留)。孤儿会话(存活但未出现在快照中的
/// 守护进程会话)会落在末尾的 "Recovered" 工作区。会话已死的窗格
/// (或非守护进程的)会被剔除,分割树围绕它们收拢。
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

    // 存活会话:id -> kind(kind 用于孤儿会话的派发)。
    let alive: Vec<(String, String)> = match daemon_client::list_sessions(&daemon_socket) {
        Ok(list) => list
            .into_iter()
            .filter_map(|v| {
                let id = v.get("session_id")?.as_str()?.to_string();
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

/// GAP-7.5:冷恢复一个 GUI 关闭期间守护进程会话死掉的 agent。
/// 从 `[[vault.agents]]` 按 kind 解析该 agent 的 `resume_command`,
/// 替换 `{session_id}`,经守护进程拉起(守护进程持有
/// 新 PTY)并附加。当 kind
/// 未配置时回退到 `claude --resume <id>`。构建于 80×24 —— 附加后由调用方调整尺寸。
#[cfg(unix)]
pub fn cold_resume_agent(
    agent_session_id: &str,
    agent_kind: Option<&str>,
) -> std::io::Result<Arc<TerminalContext>> {
    use alacritty_terminal::term::test::TermSize;

    let kind = agent_kind.unwrap_or("claude-code");
    // GAP-7.7:优先使用经 HMAC 令牌授权的 `auto` 恢复绑定
    // (例如 `tmux attach -t work`),而不是默认的 `claude --resume`。冷
    // 恢复从不提示 —— 没有令牌(或没有 auto 绑定)就直接落入
    // 下方的 vault / claude 路径。
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
        // 默认:`claude --resume <id>`(最常见的 agent)。
        vec!["claude".into(), "--resume".into(), agent_session_id.to_string()]
    });
    // 在 argv 的每个元素中替换 `{session_id}` 占位符。
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

/// 把某个窗格的终端上下文附加到存活的守护进程会话。本地与
/// SSH 都经由 `attach_existing` → `DaemonPty`;SSH 复用存活的
/// `ssh -tt` 子进程(无需重新认证)。以 80×24 占位构建 —— 调用方
/// 附加后立即调整到真实窗口尺寸。
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

/// 为原始 profile 已从配置删除(或从未持久化)的 SSH 会话提供的
/// 最小占位 profile。远端 shell 继续
/// 通过守护进程运行;只有显示名 + 重连元数据是
/// 尽力而为的。
#[cfg(unix)]
fn synthetic_ssh_profile(name: Option<&str>) -> SshProfile {
    SshProfile {
        name: name.unwrap_or("SSH").to_string(),
        ..Default::default()
    }
}
