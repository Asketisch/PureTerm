//! GAP-59: Emoji dedicated rendering channel scaffold.

/// `[emoji]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmojiRenderingConfig {
    pub enabled: bool,
    pub dedicated_atlas: bool,
    pub font_fallback: String,
}

impl Default for EmojiRenderingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dedicated_atlas: false,
            font_fallback: "Apple Color Emoji".into(),
        }
    }
}

impl EmojiRenderingConfig {
    pub fn use_dedicated_channel(&self) -> bool {
        self.enabled && self.dedicated_atlas
    }

    /// Detect ZWJ emoji sequence (basic MVP heuristic).
    pub fn is_complex_emoji(s: &str) -> bool {
        s.contains('\u{200d}') || s.chars().count() > 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zwj_detected() {
        assert!(EmojiRenderingConfig::is_complex_emoji("👨‍💻"));
    }
}
