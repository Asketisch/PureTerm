//! GAP-13: Broadcast Input — send keystrokes to multiple terminal panes.
//!
//! Toggle via `ToggleBroadcastInput` (default ⌘⌃B). Scope: current tab / all tabs
//! in the active window. Status bar shows `BROADCAST` when active.

use crate::iced_app::{Tab, TabId, Window};
use crate::split_tree::{Pane, PaneId, Surface};

/// `[broadcast_input]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BroadcastConfig {
    pub enabled: bool,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Which panes receive duplicated keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BroadcastScope {
    /// All terminal panes in the active tab (excluding Web panes).
    #[default]
    CurrentTab,
    /// All terminal panes in every tab of the active window.
    AllTabs,
}

/// Runtime broadcast state (config gate in [`BroadcastConfig`]).
#[derive(Debug, Clone, Default)]
pub struct BroadcastState {
    pub enabled: bool,
    pub scope: BroadcastScope,
}

impl BroadcastState {
    pub fn toggle(&mut self) -> bool {
        self.enabled = !self.enabled;
        self.enabled
    }
}

/// Collect terminal pane ids that should receive broadcast keystrokes.
pub fn collect_target_panes(
    window: &Window,
    active_tab: TabId,
    scope: BroadcastScope,
    exclude: PaneId,
) -> Vec<PaneId> {
    let tabs: Vec<&Tab> = match scope {
        BroadcastScope::CurrentTab => window
            .active_workspace()
            .and_then(|ws| ws.tabs.iter().find(|t| t.id == active_tab))
            .into_iter()
            .collect(),
        BroadcastScope::AllTabs => window
            .active_workspace()
            .map(|ws| ws.tabs.iter().collect())
            .unwrap_or_default(),
    };

    let mut out = Vec::new();
    for tab in tabs {
        for pane in tab.panes.values() {
            if pane.id == exclude {
                continue;
            }
            if pane.terminal().is_some() {
                out.push(pane.id);
            }
        }
    }
    out
}

/// Write `bytes` to every pane in `targets` (best-effort).
pub fn write_to_panes(tabs: &[Tab], targets: &[PaneId], bytes: &[u8]) {
    for tab in tabs {
        for id in targets {
            if let Some(pane) = tab.panes.get(id) {
                if let Some(ctx) = pane.terminal() {
                    ctx.write_to_pty(bytes);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_flips_enabled() {
        let mut st = BroadcastState::default();
        assert!(!st.enabled);
        assert!(st.toggle());
        assert!(st.enabled);
        assert!(!st.toggle());
    }
}
