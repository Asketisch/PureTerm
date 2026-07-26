//! 单 tab 恢复 —— `restore_one_tab`(遍历 SplitNode → 创建 pane/pty/ssh)。

use std::sync::Arc;

use super::{RestoredPane, RestoredTab, TabSnapshot};
use super::restore::{attach_pane, cold_resume_agent};
use crate::ssh::config::SshProfile;
use crate::split_tree::{PaneId, SplitNode};
use crate::terminal_context::TerminalContext;

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub(super) fn restore_one_tab(
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
