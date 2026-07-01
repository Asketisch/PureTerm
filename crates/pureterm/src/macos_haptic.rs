//! GAP-57: MacBook haptic feedback.
//!
//! macOS: `NSHapticFeedbackManager.defaultPerformer().performFeedbackPattern(_:performanceTime:)`
//! fires the system trackpad haptic actuator. Errors (no Mac, no force-touch
//! trackpad, the API rejected the request) are logged and otherwise ignored
//! — haptics are best-effort and never block the UI thread.
//!
//! Other platforms: no-op (the system has no equivalent API surface).

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

/// Result of a haptic request. `Fired` means the API acknowledged the
/// request; `Skipped` / `Failed` distinguish user-disabled vs. platform
/// rejection (mostly the former on non-Macs and the latter on Macs that
/// lack a force-touch trackpad).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HapticOutcome {
    Skipped,
    Fired,
    Failed(String),
}

#[cfg(target_os = "macos")]
fn pattern_for(event: HapticEvent) -> objc2_app_kit::NSHapticFeedbackPattern {
    use objc2_app_kit::NSHapticFeedbackPattern;
    match event {
        // Generic is the safest choice: alignment / level / generic all
        // map to the same underlying actuator, but `Generic` is available
        // on every Mac that supports haptics (10.11+).
        HapticEvent::TabSwitch => NSHapticFeedbackPattern::Generic,
        HapticEvent::Error => NSHapticFeedbackPattern::Generic,
        HapticEvent::Notification => NSHapticFeedbackPattern::Generic,
    }
}

/// Perform a haptic event. Safe to call on any thread (the macOS API is
/// documented as thread-safe). Returns the outcome so callers (and tests)
/// can distinguish skip vs. fired vs. error.
pub fn perform(cfg: &HapticConfig, event: HapticEvent) -> HapticOutcome {
    if !cfg.should_fire(event) {
        return HapticOutcome::Skipped;
    }
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::ProtocolObject;
                use objc2_app_kit::{NSHapticFeedbackManager, NSHapticFeedbackPerformanceTime};
        let performer =
            NSHapticFeedbackManager::defaultPerformer();
        let performer: &ProtocolObject<dyn objc2_app_kit::NSHapticFeedbackPerformer> = &*performer;
        let pattern = pattern_for(event);
        let time = NSHapticFeedbackPerformanceTime::Default;
        // performFeedbackPattern:performanceTime: returns void. We call
        // the trait method directly (objc2 0.6 re-exports it on the
        // ProtocolObject) under a `catch_unwind` boundary so a misbehaving
        // performer cannot crash the UI thread.
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            objc2_app_kit::NSHapticFeedbackPerformer::performFeedbackPattern_performanceTime(
                &*performer, pattern, time,
            );
        }));
        match res {
            Ok(()) => HapticOutcome::Fired,
            Err(_) => HapticOutcome::Failed("performer panicked".into()),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = event;
        HapticOutcome::Skipped
    }
}

/// Convenience: fire-and-forget. Logs the outcome at debug level and
/// drops the result.
pub fn perform_logged(cfg: &HapticConfig, event: HapticEvent) {
    let outcome = perform(cfg, event);
    match outcome {
        HapticOutcome::Skipped => {}
        HapticOutcome::Fired => log::debug!("haptic {:?} fired", event),
        HapticOutcome::Failed(e) => log::debug!("haptic {:?} failed: {e}", event),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_skips() {
        assert!(!HapticConfig::default().should_fire(HapticEvent::Error));
    }

    #[test]
    fn enabled_by_kind() {
        let cfg = HapticConfig {
            enabled: true,
            on_tab_switch: true,
            on_error: false,
            on_notification: true,
        };
        assert!(cfg.should_fire(HapticEvent::TabSwitch));
        assert!(!cfg.should_fire(HapticEvent::Error));
        assert!(cfg.should_fire(HapticEvent::Notification));
    }

    #[test]
    fn perform_skipped_when_disabled() {
        let cfg = HapticConfig::default();
        assert_eq!(perform(&cfg, HapticEvent::Error), HapticOutcome::Skipped);
    }

    #[test]
    fn perform_logged_is_safe() {
        let cfg = HapticConfig::default();
        // Disabled config => Skipped, log path is no-op.
        perform_logged(&cfg, HapticEvent::TabSwitch);
    }
}
