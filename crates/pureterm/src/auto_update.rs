//! GAP-45: Auto-update — Sparkle / GitHub Releases check scaffold.

/// `[update]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoUpdateConfig {
    pub enabled: bool,
    pub auto_check: bool,
    /// `stable` | `beta`
    pub channel: String,
    pub feed_url: String,
}

impl Default for AutoUpdateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_check: false,
            channel: "stable".into(),
            feed_url: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_available: Option<String>,
    pub message: String,
}

/// Check for updates. MVP: compare against env `PURETERM_LATEST_VERSION` or report current only.
pub fn check_for_updates(cfg: &AutoUpdateConfig) -> UpdateCheckResult {
    let current = env!("CARGO_PKG_VERSION").to_string();
    if !cfg.enabled {
        return UpdateCheckResult {
            current_version: current.clone(),
            latest_available: None,
            message: "update checks disabled".into(),
        };
    }
    let latest = std::env::var("PURETERM_LATEST_VERSION").ok();
    let message = match &latest {
        Some(v) if v != &current => format!("update available: {v} (current {current})"),
        Some(_) => format!("up to date ({current})"),
        None => {
            if cfg.feed_url.is_empty() {
                format!("current {current}; configure update.feed_url for remote check")
            } else {
                format!("current {current}; remote feed check deferred (Sparkle pending)")
            }
        }
    };
    UpdateCheckResult {
        current_version: current,
        latest_available: latest,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_reports_current() {
        let r = check_for_updates(&AutoUpdateConfig::default());
        assert_eq!(r.current_version, env!("CARGO_PKG_VERSION"));
    }
}
