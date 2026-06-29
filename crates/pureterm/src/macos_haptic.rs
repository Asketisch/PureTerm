//! GAP-57: MacBook haptic feedback on key events.

/// Haptic event kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HapticEvent {
    TabSwitch,
    Error,
    Notification,
}

/// `[haptic]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HapticConfig {
    pub enabled: bool,
    pub on_tab_switch: bool,
    pub on_error: bool,
    pub on_notification: bool,
}

impl Default for HapticConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            on_tab_switch: true,
            on_error: true,
            on_notification: true,
        }
    }
}

impl HapticConfig {
    pub fn should_fire(&self, event: HapticEvent) -> bool {
        if !self.enabled {
            return false;
        }
        match event {
            HapticEvent::TabSwitch => self.on_tab_switch,
            HapticEvent::Error => self.on_error,
            HapticEvent::Notification => self.on_notification,
        }
    }
}

/// Perform haptic feedback. MVP stub.
pub fn perform(cfg: &HapticConfig, event: HapticEvent) {
    if !cfg.should_fire(event) {
        return;
    }
    #[cfg(target_os = "macos")]
    log::debug!("GAP-57: haptic {:?} (NSHapticFeedbackManager pending)", event);
    #[cfg(not(target_os = "macos"))]
    let _ = event;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_skips() {
        assert!(!HapticConfig::default().should_fire(HapticEvent::Error));
    }
}
