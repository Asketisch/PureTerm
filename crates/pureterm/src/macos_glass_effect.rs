//! GAP-20: macOS window glass / vibrancy effect.

/// `[window.glass]` config.
#[derive(Clone, Debug, PartialEq)]
pub struct GlassEffectConfig {
    pub enabled: bool,
    /// `glass` (macOS 26+) or `vibrancy` fallback.
    pub style: String,
}

impl Default for GlassEffectConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            style: "vibrancy".into(),
        }
    }
}

/// Apply glass/vibrancy to the main window. MVP: log + no-op until NSVisualEffect wired.
pub fn apply_glass_effect(cfg: &GlassEffectConfig) {
    if !cfg.enabled {
        return;
    }
    #[cfg(target_os = "macos")]
    log::info!(
        "GAP-20: glass effect requested (style={}); NSVisualEffectView hook pending",
        cfg.style
    );
    #[cfg(not(target_os = "macos"))]
    log::debug!("GAP-20: glass effect ignored on this platform");
}
