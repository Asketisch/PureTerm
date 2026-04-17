//! 终端背景图片支持。

use serde::{Deserialize, Serialize};

/// 终端背景图片的配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundConfig {
    /// 背景图片文件路径。
    pub image_path: Option<String>,
    /// 图片渲染方式。
    #[serde(default)]
    pub mode: BackgroundMode,
    /// 背景图片不透明度(0.0 = 不可见,1.0 = 完全不透明)。
    #[serde(default = "default_bg_opacity")]
    pub opacity: f32,
    /// 是否模糊背景图片。
    #[serde(default)]
    pub blur: bool,
}

fn default_bg_opacity() -> f32 { 0.3 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundMode {
    /// 拉伸以填充窗口。
    Stretch,
    /// 平铺图片。
    Tile,
    /// 以原始尺寸居中显示图片。
    Center,
    /// 缩放以覆盖窗口,同时保持宽高比。
    Cover,
    /// 缩放以适应窗口,同时保持宽高比。
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

    /// 是否配置了背景图片。
    pub fn has_image(&self) -> bool {
        self.image_path.is_some()
    }

    /// 加载背景图片并返回 RGBA 像素数据。
    /// 返回 (宽度, 高度, rgba 数据)。
    pub fn load_image(&self) -> Option<(u32, u32, Vec<u8>)> {
        let path = self.image_path.as_ref()?;
        let img = image::open(path).ok()?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        Some((w, h, rgba.into_raw()))
    }

    /// 根据给定模式计算 UV 坐标。
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
