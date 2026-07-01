//! GAP-54: Terminal background image — config + decode + GPU-ready RGBA buffer.

use std::path::{Path, PathBuf};

/// Image tiling mode (positioning within the terminal viewport).
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

/// `[background]` config.
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

/// Decoded RGBA8 image data ready to be uploaded to a `wgpu::Texture`.
/// Width / height in pixels; data length is exactly `width * height * 4`.
#[derive(Clone, Debug)]
pub struct DecodedBackground {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Decode a PNG/JPEG file into a top-down RGBA8 buffer. Errors are
/// returned as `String` (this module is called from non-wgpu contexts;
/// callers translate as needed).
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

/// Convenience: resolve the config + decode in one step. Returns
/// `Ok(None)` when the config disables the background or has no file;
/// `Ok(Some(DecodedBackground))` on success; `Err` on decode failure.
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
        // resolved_path requires the file to exist on disk, so it's None
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
