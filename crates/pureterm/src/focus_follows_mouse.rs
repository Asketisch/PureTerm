//! GAP-56:类 X11 的“鼠标跟随聚焦”,作用于拆分窗格。

use std::time::{Duration, Instant};

/// `[focus_follows_mouse]` 配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusFollowsMouseConfig {
    pub enabled: bool,
    /// 悬停后切换聚焦前的延迟(毫秒)。
    pub delay_ms: u64,
}

impl Default for FocusFollowsMouseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            delay_ms: 150,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FocusFollowsMouseState {
    pub hover_pane: Option<u64>,
    pub hover_since: Option<Instant>,
}

impl FocusFollowsMouseState {
    pub fn on_pane_enter(&mut self, pane_id: u64) {
        self.hover_pane = Some(pane_id);
        self.hover_since = Some(Instant::now());
    }

    pub fn pane_to_focus(&mut self, cfg: &FocusFollowsMouseConfig) -> Option<u64> {
        if !cfg.enabled {
            return None;
        }
        let pane = self.hover_pane?;
        let since = self.hover_since?;
        if since.elapsed() >= Duration::from_millis(cfg.delay_ms) {
            Some(pane)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_never_focuses() {
        let mut st = FocusFollowsMouseState::default();
        st.on_pane_enter(1);
        assert!(st.pane_to_focus(&FocusFollowsMouseConfig::default()).is_none());
    }
}
