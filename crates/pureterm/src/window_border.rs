//! GAP-28: Custom window border (active / inactive styles).

/// `[window.border]` config.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowBorderConfig {
    pub enabled: bool,
    pub width: f32,
    pub active_color: String,
    pub inactive_color: String,
}

impl Default for WindowBorderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            width: 2.0,
            active_color: "#007AFF".into(),
            inactive_color: "#555555".into(),
        }
    }
}

impl WindowBorderConfig {
    pub fn color_for_focus(&self, focused: bool) -> &str {
        if focused {
            &self.active_color
        } else {
            &self.inactive_color
        }
    }
}
