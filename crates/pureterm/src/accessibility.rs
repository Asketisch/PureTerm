//! GAP-4.6: VoiceOver / accessibility announcements scaffold.

/// `[accessibility]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityConfig {
    pub enabled: bool,
    pub announce_tab_changes: bool,
    pub announce_notifications: bool,
}

impl Default for AccessibilityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            announce_tab_changes: true,
            announce_notifications: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AccessibilityState {
    pub last_announcement: Option<String>,
}

impl AccessibilityState {
    pub fn announce(&mut self, cfg: &AccessibilityConfig, message: &str) {
        if !cfg.enabled || message.is_empty() {
            return;
        }
        self.last_announcement = Some(message.to_string());
        #[cfg(target_os = "macos")]
        log::info!("AX announce: {message}");
        #[cfg(not(target_os = "macos"))]
        log::debug!("accessibility: {message}");
    }

    pub fn label_for_tab(&self, index: usize, title: &str) -> String {
        format!("Tab {} of {}, {}", index + 1, title, title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_announcement() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce(&cfg, "hello");
        assert_eq!(st.last_announcement.as_deref(), Some("hello"));
    }
}
