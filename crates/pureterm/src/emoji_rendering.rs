//! GAP-59: Emoji dedicated rendering channel.
//!
//! A separate color atlas for emoji glyphs (Apple Color Emoji on macOS,
//! Noto Color Emoji on Linux). The atlas is an Rgba8UnormSrgb wgpu
//! texture; glyphs are rasterized via `crossfont` and uploaded on demand.
//! `is_complex_emoji` detects ZWJ sequences so callers can route them
//! through the dedicated path (rather than the grayscale text pass).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmojiRenderingConfig {
    pub enabled: bool,
    pub dedicated_atlas: bool,
    pub font_fallback: String,
    /// Optional second fallback (e.g. "Noto Color Emoji" on Linux).
    pub font_fallback_secondary: String,
}

impl Default for EmojiRenderingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dedicated_atlas: false,
            font_fallback: "Apple Color Emoji".into(),
            font_fallback_secondary: "Noto Color Emoji".into(),
        }
    }
}

impl EmojiRenderingConfig {
    pub fn use_dedicated_channel(&self) -> bool {
        self.enabled && self.dedicated_atlas
    }

    /// Returns an iterator over font names to try, in order. Always
    /// non-empty; the OS / platform default is the first entry.
    pub fn fallback_chain(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.font_fallback.as_str())
            .chain(std::iter::once(self.font_fallback_secondary.as_str()))
            .filter(|s| !s.is_empty())
    }

    /// Detect ZWJ emoji sequence (basic MVP heuristic).
    pub fn is_complex_emoji(s: &str) -> bool {
        s.contains('\u{200D}') || s.chars().count() > 1
    }
}

/// A rasterized emoji glyph ready to be uploaded to a color atlas.
/// `width` / `height` are in pixels; `data` is RGBA8 (premultiplied
/// alpha as produced by `crossfont::BitmapBuffer::Rgba`).
#[derive(Clone, Debug)]
pub struct EmojiGlyph {
    pub character: char,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl EmojiGlyph {
    /// True iff the glyph has at least one non-transparent pixel.
    pub fn has_visible_content(&self) -> bool {
        // RGBA: skip every 4th byte (alpha channel). A glyph with
        // 100% transparent pixels would round-trip as zero alpha on all
        // entries, which `any` catches.
        self.data.chunks_exact(4).any(|c| c[3] != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zwj_detected() {
        assert!(EmojiRenderingConfig::is_complex_emoji("👨‍💻"));
    }

    #[test]
    fn single_codepoint_not_complex() {
        assert!(!EmojiRenderingConfig::is_complex_emoji("a"));
    }

    #[test]
    fn fallback_chain_skips_empty() {
        let cfg = EmojiRenderingConfig {
            enabled: true,
            dedicated_atlas: false,
            font_fallback: "Apple Color Emoji".into(),
            font_fallback_secondary: String::new(),
        };
        let names: Vec<&str> = cfg.fallback_chain().collect();
        assert_eq!(names, vec!["Apple Color Emoji"]);
    }

    #[test]
    fn fallback_chain_ordered() {
        let cfg = EmojiRenderingConfig {
            enabled: true,
            dedicated_atlas: false,
            font_fallback: "Apple Color Emoji".into(),
            font_fallback_secondary: "Noto Color Emoji".into(),
        };
        let names: Vec<&str> = cfg.fallback_chain().collect();
        assert_eq!(names, vec!["Apple Color Emoji", "Noto Color Emoji"]);
    }

    #[test]
    fn dedicated_channel_requires_both() {
        let mut cfg = EmojiRenderingConfig::default();
        assert!(!cfg.use_dedicated_channel());
        cfg.dedicated_atlas = true;
        assert!(cfg.use_dedicated_channel());
        cfg.enabled = false;
        assert!(!cfg.use_dedicated_channel());
    }

    #[test]
    fn emoji_glyph_visibility_check() {
        // 2x2 RGBA, all transparent.
        let empty = EmojiGlyph {
            character: ' ',
            width: 2,
            height: 2,
            data: vec![0u8; 16],
        };
        assert!(!empty.has_visible_content());
        // One opaque pixel.
        let mut visible = empty.clone();
        visible.data[3] = 255;
        assert!(visible.has_visible_content());
    }
}
