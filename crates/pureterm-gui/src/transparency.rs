//! Window transparency and blur effects.

use serde::{Deserialize, Serialize};

/// Configuration for window transparency and blur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransparencyConfig {
    /// Window opacity (0.0 = fully transparent, 1.0 = opaque).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Whether to enable blur behind the window.
    #[serde(default)]
    pub blur: bool,
    /// Blur radius in pixels (if supported by platform).
    #[serde(default = "default_blur_radius")]
    pub blur_radius: u32,
    /// Whether to use transparent background (without blur).
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

    /// Apply transparency to a wgpu surface configuration.
    pub fn configure_surface(&self, config: &mut wgpu::SurfaceConfiguration) {
        if self.opacity < 1.0 || self.transparent_background {
            config.alpha_mode = wgpu::CompositeAlphaMode::PostMultiplied;
        }
    }

    /// Get the clear color with opacity applied.
    pub fn clear_color(&self, bg: [u8; 3]) -> wgpu::Color {
        let alpha = self.opacity as f64;
        wgpu::Color {
            r: bg[0] as f64 / 255.0 * alpha,
            g: bg[1] as f64 / 255.0 * alpha,
            b: bg[2] as f64 / 255.0 * alpha,
            a: alpha,
        }
    }

    /// Check if blur is supported on the current platform.
    pub fn is_blur_supported() -> bool {
        #[cfg(target_os = "macos")]
        { true }
        #[cfg(not(target_os = "macos"))]
        { false }
    }

    /// Request window blur (macOS: NSVisualEffectView, Linux: KWin blur).
    /// This returns platform-specific hints for the window manager.
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

/// Platform hint for window blur.
#[derive(Debug, Clone)]
pub struct BlurHint {
    pub enabled: bool,
    pub radius: u32,
}
