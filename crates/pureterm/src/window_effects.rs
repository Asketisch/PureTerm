//! GAP-55: Window transparency and blur.

/// `[window.effects]` config (opacity + blur).
#[derive(Clone, Debug, PartialEq)]
pub struct WindowEffectsConfig {
    pub enabled: bool,
    pub opacity: f32,
    pub blur: bool,
}

impl Default for WindowEffectsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            opacity: 1.0,
            blur: false,
        }
    }
}

impl WindowEffectsConfig {
    pub fn effective_opacity(&self, base: f32) -> f32 {
        if !self.enabled {
            return base;
        }
        (base * self.opacity).clamp(0.05, 1.0)
    }
}

/// Apply blur hint on macOS. MVP: log only.
pub fn apply_blur(cfg: &WindowEffectsConfig) {
    if !cfg.enabled || !cfg.blur {
        return;
    }
    #[cfg(target_os = "macos")]
    log::info!("GAP-55: window blur requested (NSVisualEffectView hook pending)");
    #[cfg(not(target_os = "macos"))]
    log::debug!("GAP-55: blur not supported on this platform");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_opacity() {
        let cfg = WindowEffectsConfig {
            enabled: true,
            opacity: 2.0,
            blur: false,
        };
        assert_eq!(cfg.effective_opacity(1.0), 1.0);
    }
}
