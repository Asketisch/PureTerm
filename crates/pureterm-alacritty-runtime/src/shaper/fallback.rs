//! T-core-shaper: Font fallback chain for per-glyph font selection.
//!
//! The fallback chain maps Unicode script ranges to specific font faces,
//! enabling proper rendering of CJK, emoji, and complex scripts without
//! requiring a single monolithic font file.
//!
//! Default chain (from spec):
//! 1. Monospace (primary) — Latin / ASCII / basic symbols
//! 2. CJK JP — Hiragana, Katakana, Japanese Kanji
//! 3. CJK KR — Hangul, Korean Hanja
//! 4. CJK SC — Simplified Chinese
//! 5. Emoji — Color emoji (ZWJ sequences, modifiers)
//! 6. Serif — Fallback for rare characters

use std::sync::Arc;

use super::{rustybuzz_shaper::FontFace, RustyBuzzShaper, RustyBuzzConfig};

/// A named font face in the fallback chain.
#[derive(Clone, Debug)]
pub struct NamedFontFace {
    /// Human-readable name (e.g., "Monospace", "CJK JP").
    pub name: String,
    /// The actual font data.
    pub face: FontFace,
    /// Unicode script this face is optimized for.
    pub script: FallbackScript,
}

/// The supported fallback scripts in priority order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FallbackScript {
    /// Primary monospace font (Latin, ASCII, basic symbols).
    Monospace,
    /// Japanese fonts (Hiragana, Katakana, Kanji).
    CjkJp,
    /// Korean fonts (Hangul, Hanja).
    CjkKr,
    /// Simplified Chinese fonts.
    CjkSc,
    /// Color emoji font.
    Emoji,
    /// Serif fallback for rare characters.
    Serif,
}

impl FallbackScript {
    /// Get the display name of this script.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Monospace => "monospace",
            Self::CjkJp => "cjk-jp",
            Self::CjkKr => "cjk-kr",
            Self::CjkSc => "cjk-sc",
            Self::Emoji => "emoji",
            Self::Serif => "serif",
        }
    }

    /// Get the rustybuzz script tag for this fallback.
    pub fn script_tag(&self) -> &'static str {
        match self {
            Self::Monospace => "latn",
            Self::CjkJp => "kana",
            Self::CjkKr => "hang",
            Self::CjkSc => "hani",
            Self::Emoji => "DFLT", // Emoji handled via variation selectors.
            Self::Serif => "DFLT",
        }
    }
}

/// The font fallback chain. Holds a list of named font faces ordered by
/// priority and provides methods to select the best face for a given input.
#[derive(Clone, Debug)]
pub struct FallbackChain {
    /// Ordered list of font faces.
    pub faces: Vec<NamedFontFace>,
}

impl Default for FallbackChain {
    fn default() -> Self {
        // Build the default chain with no actual fonts (stub).
        // In production, these would be loaded from system fonts or bundled.
        Self { faces: Vec::new() }
    }
}

impl FallbackChain {
    /// Create a new fallback chain with the given named font faces.
    pub fn new(faces: Vec<NamedFontFace>) -> Self {
        Self { faces }
    }

    /// Get the primary (first) font face. This is used for Latin/ASCII text.
    pub fn primary(&self) -> Option<&FontFace> {
        self.faces.first().map(|f| &f.face)
    }

    /// Get a font face by script type.
    pub fn by_script(&self, script: FallbackScript) -> Option<&FontFace> {
        self.faces.iter()
            .find(|f| f.script == script)
            .map(|f| &f.face)
    }

    /// Build a RustyBuzzConfig from this fallback chain.
    pub fn build_config(&self, language: String) -> RustyBuzzConfig {
        let primary = self.primary().cloned();
        let mut fallbacks = Vec::new();
        for face in self.faces.iter().skip(1) {
            fallbacks.push(face.face.clone());
        }

        RustyBuzzConfig {
            font_face: primary,
            fallback_faces: fallbacks,
            language,
            enable_features: true,
        }
    }

    /// Create a shaper from this fallback chain.
    pub fn create_shaper(&self, language: String) -> RustyBuzzShaper {
        let config = self.build_config(language);
        RustyBuzzShaper::new(config)
    }
}

/// A builder for constructing fallback chains with specific font data.
#[derive(Debug)]
pub struct FallbackChainBuilder {
    faces: Vec<NamedFontFace>,
    language: String,
}

impl Default for FallbackChainBuilder {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            language: "en-US".to_string(),
        }
    }
}

impl FallbackChainBuilder {
    /// Set the primary monospace font.
    pub fn with_monospace(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "Monospace".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::Monospace,
        });
        self
    }

    /// Add a CJK JP font.
    pub fn with_cjk_jp(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "CJK JP".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::CjkJp,
        });
        self
    }

    /// Add a CJK KR font.
    pub fn with_cjk_kr(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "CJK KR".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::CjkKr,
        });
        self
    }

    /// Add a CJK SC font.
    pub fn with_cjk_sc(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "CJK SC".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::CjkSc,
        });
        self
    }

    /// Add an emoji font.
    pub fn with_emoji(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "Emoji".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::Emoji,
        });
        self
    }

    /// Add a serif fallback font.
    pub fn with_serif(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "Serif".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::Serif,
        });
        self
    }

    /// Set the language for shaping.
    pub fn with_language(mut self, lang: String) -> Self {
        self.language = lang;
        self
    }

    /// Build the fallback chain.
    pub fn build(self) -> FallbackChain {
        FallbackChain::new(self.faces)
    }

    /// Build a shaper directly.
    pub fn create_shaper(self) -> RustyBuzzShaper {
        let language = self.language.clone();
        let chain = self.build();
        chain.create_shaper(language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shaper::Shaper;

    #[test]
    fn test_default_chain_is_empty() {
        let chain = FallbackChain::default();
        assert!(chain.faces.is_empty());
        assert!(chain.primary().is_none());
    }

    #[test]
    fn test_build_config_from_chain() {
        // With no fonts, the config should have None primary and empty fallbacks.
        let chain = FallbackChain::default();
        let config = chain.build_config("en-US".to_string());
        assert!(config.font_face.is_none());
        assert!(config.fallback_faces.is_empty());
        assert_eq!(config.language, "en-US");
    }

    #[test]
    fn test_by_script_returns_correct_face() {
        let data = Arc::new(vec![0u8; 100]); // dummy font data.
        let chain = FallbackChainBuilder::default()
            .with_monospace(data.clone())
            .with_cjk_jp(data.clone())
            .build();

        assert!(chain.by_script(FallbackScript::Monospace).is_some());
        assert!(chain.by_script(FallbackScript::CjkJp).is_some());
        assert!(chain.by_script(FallbackScript::Emoji).is_none());
    }

    #[test]
    fn test_script_names() {
        assert_eq!(FallbackScript::Monospace.name(), "monospace");
        assert_eq!(FallbackScript::CjkJp.name(), "cjk-jp");
        assert_eq!(FallbackScript::Emoji.name(), "emoji");
    }

    #[test]
    fn test_script_tags() {
        assert_eq!(FallbackScript::Monospace.script_tag(), "latn");
        assert_eq!(FallbackScript::CjkJp.script_tag(), "kana");
        assert_eq!(FallbackScript::CjkKr.script_tag(), "hang");
    }

    #[test]
    fn test_builder_creates_shaper() {
        // Even without real fonts, the builder should produce a valid shaper.
        let shaper = FallbackChainBuilder::default().create_shaper();
        let run = shaper.shape("hello", 7.0);
        assert_eq!(run.glyphs.len(), 5); // Monospace fallback.
    }
}
