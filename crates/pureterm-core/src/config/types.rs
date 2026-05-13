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
