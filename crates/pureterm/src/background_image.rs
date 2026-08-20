//! GAP-54:终端背景图——配置 + 解码 + 供 GPU 使用的 RGBA 缓冲。

use std::path::{Path, PathBuf};

/// 图片平铺模式(在终端视口内的定位方式)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BackgroundMode {
    #[default]
    Center,
    Tile,
    Stretch,
}

impl BackgroundMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "tile" => Self::Tile,
            "stretch" => Self::Stretch,
            _ => Self::Center,
        }
    }
}

/// `[background]` 配置。
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundImageConfig {
    pub enabled: bool,
    pub image: PathBuf,
    pub opacity: f32,
    pub mode: BackgroundMode,
}

impl Default for BackgroundImageConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            image: PathBuf::new(),
            opacity: 0.35,
            mode: BackgroundMode::Center,
        }
    }
}

impl BackgroundImageConfig {
    pub fn resolved_path(&self) -> Option<&Path> {
        if !self.enabled || self.image.as_os_str().is_empty() {
            return None;
        }
        if self.image.exists() {
            Some(&self.image)
        } else {
            None
        }
    }
}

/// 解码后的 RGBA8 图像数据,准备上传到 `wgpu::Texture`。
/// 宽 / 高以像素计;数据长度恰为 `width * height * 4`。
#[derive(Clone, Debug)]
pub struct DecodedBackground {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// 把 PNG/JPEG 文件解码为自上而下的 RGBA8 缓冲。错误以
/// `String` 形式返回(本模块会从非 wgpu 上下文调用;
/// 调用方按需转换)。
pub fn decode_rgba8(path: &Path) -> Result<DecodedBackground, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    decode_rgba8_from_bytes(&bytes)
}

pub fn decode_rgba8_from_bytes(bytes: &[u8]) -> Result<DecodedBackground, String> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("decode image: {e}"))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(DecodedBackground {
        width: w,
        height: h,
        data: rgba.into_raw(),
    })
}

/// 便捷封装:一步完成配置解析 + 解码。返回
/// `Ok(None)`(配置禁用背景或没有文件)、
/// 成功时 `Ok(Some(DecodedBackground))`;解码失败为 `Err`。
pub fn load_decoded(cfg: &BackgroundImageConfig) -> Result<Option<DecodedBackground>, String> {
    let Some(path) = cfg.resolved_path() else {
        return Ok(None);
    };
    decode_rgba8(path).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_none() {
        assert!(BackgroundImageConfig::default().resolved_path().is_none());
        assert!(load_decoded(&BackgroundImageConfig::default()).unwrap().is_none());
    }

    #[test]
    fn invalid_path_errors() {
        let cfg = BackgroundImageConfig {
            enabled: true,
            image: PathBuf::from("/nonexistent/never.png"),
            opacity: 0.5,
            mode: BackgroundMode::Center,
        };
        // resolved_path 要求文件真实存在于磁盘,因此为 None
        assert!(cfg.resolved_path().is_none());
        assert!(load_decoded(&cfg).unwrap().is_none());
    }

    #[test]
    fn decode_invalid_bytes_errors() {
        let r = decode_rgba8_from_bytes(b"not an image");
        assert!(r.is_err());
    }

    #[test]
    fn parse_modes() {
        assert_eq!(BackgroundMode::parse("center"), BackgroundMode::Center);
        assert_eq!(BackgroundMode::parse("TILE"), BackgroundMode::Tile);
        assert_eq!(BackgroundMode::parse("Stretch"), BackgroundMode::Stretch);
        assert_eq!(BackgroundMode::parse("garbage"), BackgroundMode::Center);
    }
}
