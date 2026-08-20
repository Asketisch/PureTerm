//! GAP-23:可视标签栏组件——位置 + 可见性配置。

/// 标签栏摆放位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TabBarPosition {
    #[default]
    Top,
    Bottom,
    Hidden,
}

impl TabBarPosition {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "bottom" => Self::Bottom,
            "hidden" | "none" => Self::Hidden,
            _ => Self::Top,
        }
    }
}

/// `[tab_bar]` 配置。
#[derive(Clone, Debug, PartialEq)]
pub struct VisualTabBarConfig {
    pub enabled: bool,
    pub position: TabBarPosition,
    pub height: f32,
}

impl Default for VisualTabBarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: TabBarPosition::Top,
            height: 28.0,
        }
    }
}

impl VisualTabBarConfig {
    pub fn should_show(&self, window_show_tab_bar: bool) -> bool {
        self.enabled && window_show_tab_bar && self.position != TabBarPosition::Hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_disables_show() {
        let cfg = VisualTabBarConfig {
            position: TabBarPosition::Hidden,
            ..Default::default()
        };
        assert!(!cfg.should_show(true));
    }
}
