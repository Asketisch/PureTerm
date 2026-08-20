//! GAP-13:广播输入——把按键发送到多个终端窗格。
//!
//! 经 `ToggleBroadcastInput` 切换(默认 ⌘⌃B)。范围:当前标签页 /
//! 活动窗口的全部标签页。激活时状态栏显示 `BROADCAST`。

use crate::iced_app::{Tab, TabId, Window};
use crate::split_tree::{Pane, PaneId, Surface};

/// `[broadcast_input]` 配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BroadcastConfig {
    pub enabled: bool,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// 接收重复键盘输入的窗格范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BroadcastScope {
    /// 活动标签页中的全部终端窗格(不含 Web 窗格)。
    #[default]
    CurrentTab,
    /// 活动窗口每个标签页中的全部终端窗格。
    AllTabs,
}

/// 运行时广播状态(开关由 [`BroadcastConfig`] 控制)。
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

/// 收集应接收广播按键的终端窗格 id。
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

/// 把 `bytes` 写入 `targets` 中的每个窗格(尽力而为)。
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
