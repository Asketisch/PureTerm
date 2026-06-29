//! GAP-54: Terminal background image config.

use std::path::{Path, PathBuf};

/// Image tiling mode.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_none() {
        assert!(BackgroundImageConfig::default().resolved_path().is_none());
    }
}
