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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayoutSnapshot {
    pub version: u32,
    pub viewport: ViewportSnapshot,
    pub active_tab: usize,
    pub tabs: Vec<TabSnapshot>,
}

pub const SNAPSHOT_VERSION: u32 = 1;
const SNAPSHOT_FILE: &str = "gui-sessions.json";

/// Path of the on-disk layout snapshot. Lives in the daemon's persistent,
/// owner-only (0o700) data dir, created by `pureterm_daemon::discovery::ensure_dirs`.
pub fn state_path() -> PathBuf {
    pureterm_daemon::discovery::data_dir().join(SNAPSHOT_FILE)
}

/// Load the snapshot. `None` if missing or unparseable (treated as fresh start).
pub fn load() -> Option<LayoutSnapshot> {
    let data = std::fs::read(state_path()).ok()?;
    match serde_json::from_slice::<LayoutSnapshot>(&data) {
        Ok(s) if s.version == SNAPSHOT_VERSION => Some(s),
        Ok(other) => {
            log::warn!(
                "session_restore: ignoring snapshot version {} (expected {})",
                other.version,
                SNAPSHOT_VERSION
            );
            None
        }
        Err(e) => {
            log::warn!("session_restore: parse failed: {e}");
            None
        }
    }
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

/// A restored pane: a freshly-attached terminal context + its display title.
#[cfg(unix)]
pub struct RestoredPane {
    pub ctx: Arc<TerminalContext>,
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

#[cfg(unix)]
pub struct RestoreOutcome {
    pub tabs: Vec<RestoredTab>,
    pub active_tab_index: usize,
}

/// Attempt to restore the previous layout by re-attaching to surviving daemon
/// sessions. Returns `None` if nothing could be restored (caller opens a fresh
/// tab). Orphans (daemon sessions not referenced by the snapshot) become extra
/// single-pane tabs. Panes whose session died (or were non-daemon) are pruned
/// and the split tree collapses around them.
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

    let mut restored_tabs: Vec<RestoredTab> = Vec::new();
    let mut restored_active_tab: usize = 0;

    for (tab_idx, tab_snap) in snapshot.tabs.iter().enumerate() {
        let leaf_ids = tab_snap.split_tree.pane_ids();
        if tab_snap.panes.len() != leaf_ids.len() {
            log::warn!(
                "session_restore: tab {} pane/leaf count mismatch ({} != {}); skipping tab",
                tab_idx,
                tab_snap.panes.len(),
                leaf_ids.len()
            );
            continue;
        }

        let mut tree = tab_snap.split_tree.clone();
        let mut alive_panes: Vec<(PaneId, RestoredPane)> = Vec::new();
        let active_old_id = leaf_ids.get(tab_snap.active_pane_leaf_index).copied();
        let mut active_survived_old_id: Option<PaneId> = None;

        for (i, old_id) in leaf_ids.iter().enumerate() {
            let pane_snap = &tab_snap.panes[i];
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
                                RestoredPane { ctx, title: pane_snap.title.clone() },
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
            match attach_pane(
                sid,
                &pane_snap.kind,
                pane_snap.ssh_profile.as_deref(),
                ssh_profiles,
            ) {
                Ok(ctx) => {
                    consumed.insert(sid.clone());
                    if Some(*old_id) == active_old_id {
                        active_survived_old_id = Some(*old_id);
                    }
                    alive_panes.push((
                        *old_id,
                        RestoredPane {
                            ctx,
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
            continue; // whole tab died
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

        if tab_idx == snapshot.active_tab {
            restored_active_tab = restored_tabs.len();
        }
        restored_tabs.push(RestoredTab {
            title: tab_snap.title.clone(),
            split_tree: tree,
            panes: panes_in_order,
            active_pane_index: active_index,
        });
    }

    // Orphans: alive sessions not referenced by any snapshot pane. Become extra
    // single-pane tabs so nothing the daemon is holding is silently lost.
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
                restored_tabs.push(RestoredTab {
                    title: title.clone(),
                    split_tree: SplitNode::single(fresh),
                    panes: vec![RestoredPane { ctx, title }],
                    active_pane_index: 0,
                });
            }
            Err(e) => log::warn!("session_restore: orphan attach {} failed: {e}", sid),
        }
    }

    if restored_tabs.is_empty() {
        None
    } else {
        if restored_active_tab >= restored_tabs.len() {
            restored_active_tab = 0;
        }
        Some(RestoreOutcome {
            tabs: restored_tabs,
            active_tab_index: restored_active_tab,
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

    /// Serialize → deserialize must preserve the session id and the split tree
    /// (the on-disk contract for cross-restart recovery).
    #[test]
    fn snapshot_round_trip() {
        let snap = LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 1024.0, h: 768.0 },
            active_tab: 0,
            tabs: vec![TabSnapshot {
                title: "zsh".to_string(),
                active_pane_leaf_index: 0,
                split_tree: SplitNode::single(crate::split_tree::PaneId(1)),
                panes: vec![PaneSnapshot {
                    session_id: Some("daemon-1".to_string()),
                    kind: "local".to_string(),
                    ssh_profile: None,
                    title: "zsh".to_string(),
                    agent_session_id: Some("cc-abc".to_string()),
                    agent_kind: Some("claude-code".to_string()),
                }],
            }],
        };
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: LayoutSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.version, SNAPSHOT_VERSION);
        assert_eq!(back.tabs.len(), 1);
        assert_eq!(back.tabs[0].panes[0].session_id.as_deref(), Some("daemon-1"));
        // GAP-7.5: agent fields survive the round-trip (cold-resume persistence).
        assert_eq!(back.tabs[0].panes[0].agent_session_id.as_deref(), Some("cc-abc"));
        assert_eq!(back.tabs[0].panes[0].agent_kind.as_deref(), Some("claude-code"));
        assert_eq!(
            back.tabs[0].split_tree.pane_ids(),
            vec![crate::split_tree::PaneId(1)]
        );
    }

    /// GAP-7.5: a pre-GAP-7.5 (v1) pane has no agent_session_id/agent_kind.
    /// `#[serde(default)]` must fill them with `None` rather than reject.
    #[test]
    fn pane_snapshot_v1_without_agent_fields_loads_with_defaults() {
        let json = r#"{"session_id":"s","kind":"local","ssh_profile":null,"title":"t"}"#;
        let p: PaneSnapshot = serde_json::from_str(json).expect("deserialize v1 pane");
        assert_eq!(p.session_id.as_deref(), Some("s"));
        assert!(p.agent_session_id.is_none());
        assert!(p.agent_kind.is_none());
    }

    /// A snapshot with the wrong version is rejected by `load`'s version gate.
    /// (We re-implement the same check here since `load` reads a fixed path.)
    #[test]
    fn wrong_version_round_trip_is_distinguishable() {
        let mut snap = LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 10.0, h: 10.0 },
            active_tab: 0,
            tabs: vec![],
        };
        let _ok = serde_json::to_string(&snap).unwrap();
        snap.version = SNAPSHOT_VERSION + 1;
        let json = serde_json::to_string(&snap).unwrap();
        let back: LayoutSnapshot = serde_json::from_str(&json).unwrap();
        assert_ne!(back.version, SNAPSHOT_VERSION);
    }
}
