//! 窗口透明度和模糊效果。

use serde::{Deserialize, Serialize};

/// 窗口透明度和模糊配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransparencyConfig {
    /// 窗口不透明度 (0.0 = 完全透明, 1.0 = 不透明)。
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// 是否启用窗口后面的模糊。
    #[serde(default)]
    pub blur: bool,
    /// 模糊半径(像素)(如果平台支持)。
    #[serde(default = "default_blur_radius")]
    pub blur_radius: u32,
    /// 是否使用透明背景(不带模糊)。
    #[serde(default)]
    pub transparent_background: bool,
}

fn default_opacity() -> f32 { 1.0 }
fn default_blur_radius() -> u32 { 20 }

impl Default for TransparencyConfig {
    fn default() -> Self {
        Self {
            opacity: default_opacity(),
            blur: false,
            blur_radius: default_blur_radius(),
            transparent_background: false,
        }
    }
}

impl TransparencyConfig {
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.1, 1.0),
            ..Default::default()
        }
    }

    /// 将透明度应用到 wgpu 表面配置。
    pub fn configure_surface(&self, config: &mut wgpu::SurfaceConfiguration) {
        if self.opacity < 1.0 || self.transparent_background {
            config.alpha_mode = wgpu::CompositeAlphaMode::PostMultiplied;
        }
    }

    /// 获取应用了不透明度的清除颜色。
    pub fn clear_color(&self, bg: [u8; 3]) -> wgpu::Color {
        let alpha = self.opacity as f64;
        wgpu::Color {
            r: bg[0] as f64 / 255.0 * alpha,
            g: bg[1] as f64 / 255.0 * alpha,
            b: bg[2] as f64 / 255.0 * alpha,
            a: alpha,
        }
    }

    /// 检查当前平台是否支持模糊。
    pub fn is_blur_supported() -> bool {
        #[cfg(target_os = "macos")]
        { true }
        #[cfg(not(target_os = "macos"))]
        { false }
    }

    /// 请求窗口模糊 (macOS: NSVisualEffectView, Linux: KWin blur)。
    /// 这会返回用于窗口管理器的平台特定提示。
    pub fn blur_hint(&self) -> Option<BlurHint> {
        if self.blur {
            Some(BlurHint {
                enabled: true,
                radius: self.blur_radius,
            })
        } else {
            None
        }
    }
}

/// 窗口模糊的平台提示。
#[derive(Debug, Clone)]
pub struct BlurHint {
    pub enabled: bool,
    pub radius: u32,
}
