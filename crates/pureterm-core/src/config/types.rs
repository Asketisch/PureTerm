//! Shared helper types used by both legacy and new config

use serde::{Deserialize, Serialize};

/// Glass effect style for window transparency
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlassEffectStyle {
    None,
    Regular,
    Clear,
    Light,
    Dark,
    Automatic,
}

impl Default for GlassEffectStyle {
    fn default() -> Self {
        Self::Automatic
    }
}

/// Input mode for terminal cursor positioning (Warp-style)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMode {
    /// Default: input pinned to bottom of screen
    PinnedToBottom,
    /// Input pinned to top of screen, blocks invert
    PinnedToTop,
    /// Input starts at top, pushed down by commands (waterfall style)
    Waterfall,
}

impl Default for InputMode {
    fn default() -> Self {
        Self::PinnedToBottom
    }
}
