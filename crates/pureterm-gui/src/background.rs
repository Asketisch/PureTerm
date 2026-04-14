//! Background image support for the terminal.

use serde::{Deserialize, Serialize};

/// Configuration for terminal background images.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundConfig {
    /// Path to the background image file.
    pub image_path: Option<String>,
    /// How the image should be rendered.
    #[serde(default)]
    pub mode: BackgroundMode,
    /// Opacity of the background image (0.0 = invisible, 1.0 = fully opaque).
    #[serde(default = "default_bg_opacity")]
    pub opacity: f32,
    /// Whether to blur the background image.
    #[serde(default)]
    pub blur: bool,
}

fn default_bg_opacity() -> f32 { 0.3 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundMode {
    /// Stretch to fill the window.
    Stretch,
    /// Tile the image.
    Tile,
    /// Center the image at original size.
    Center,
    /// Scale to cover the window while maintaining aspect ratio.
    Cover,
    /// Scale to fit within the window while maintaining aspect ratio.
    Fit,
}

impl Default for BackgroundMode {
    fn default() -> Self {
        Self::Cover
    }
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            image_path: None,
            mode: BackgroundMode::Cover,
            opacity: default_bg_opacity(),
            blur: false,
        }
    }
}

impl BackgroundConfig {
    pub fn new(path: &str) -> Self {
        Self {
            image_path: Some(path.to_string()),
            ..Default::default()
        }
    }

    /// Whether a background image is configured.
    pub fn has_image(&self) -> bool {
        self.image_path.is_some()
    }

    /// Load the background image and return RGBA pixel data.
    /// Returns (width, height, rgba_data).
    pub fn load_image(&self) -> Option<(u32, u32, Vec<u8>)> {
        let path = self.image_path.as_ref()?;
        let img = image::open(path).ok()?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        Some((w, h, rgba.into_raw()))
    }

    /// Calculate UV coordinates for the given mode.
    pub fn calculate_uv(&self, img_w: u32, img_h: u32, win_w: u32, win_h: u32) -> (f32, f32, f32, f32) {
        match self.mode {
            BackgroundMode::Stretch => (0.0, 0.0, 1.0, 1.0),
            BackgroundMode::Tile => (0.0, 0.0, win_w as f32 / img_w as f32, win_h as f32 / img_h as f32),
            BackgroundMode::Center => {
                let u1 = ((win_w as f32 - img_w as f32) / 2.0).max(0.0) / img_w as f32;
                let v1 = ((win_h as f32 - img_h as f32) / 2.0).max(0.0) / img_h as f32;
                let u2 = u1 + (win_w.min(img_w) as f32 / img_w as f32);
                let v2 = v1 + (win_h.min(img_h) as f32 / img_h as f32);
                (u1, v1, u2, v2)
            }
            BackgroundMode::Cover => {
                let scale = (win_w as f32 / img_w as f32).max(win_h as f32 / img_h as f32);
                let sw = win_w as f32 / scale;
                let sh = win_h as f32 / scale;
                let ox = (img_w as f32 - sw) / 2.0 / img_w as f32;
                let oy = (img_h as f32 - sh) / 2.0 / img_h as f32;
                (ox, oy, ox + sw / img_w as f32, oy + sh / img_h as f32)
            }
            BackgroundMode::Fit => {
                let scale = (win_w as f32 / img_w as f32).min(win_h as f32 / img_h as f32);
                let sw = win_w as f32 / scale;
                let sh = win_h as f32 / scale;
                let ox = (img_w as f32 - sw) / 2.0 / img_w as f32;
                let oy = (img_h as f32 - sh) / 2.0 / img_h as f32;
                (ox, oy, ox + sw / img_w as f32, oy + sh / img_h as f32)
            }
        }
    }
}
