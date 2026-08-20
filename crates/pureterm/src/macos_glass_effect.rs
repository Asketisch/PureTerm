//! GAP-20:macOS 窗口毛玻璃 / 活力效果。

/// `[window.glass]` 配置。
#[derive(Clone, Debug, PartialEq)]
pub struct GlassEffectConfig {
    pub enabled: bool,
    /// `glass`(macOS 26+)或 `vibrancy` 回退。
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

/// 把毛玻璃/活力效果应用到主窗口。MVP:先打日志 + 空操作,待 NSVisualEffectView 接入。
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
