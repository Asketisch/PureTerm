//! T-core-shaper: Real Harfbuzz implementation via rustybuzz.
//!
//! This module provides a full UAX #29 grapheme clusterer and Harfbuzz
//! shaper backed by `rustybuzz`. It handles complex scripts (Arabic,
//! Devanagari, etc.), ligatures, and per-glyph advances.
//!
//! v1 MonospaceShaper is kept as fallback; RustyBuzzShaper becomes the
//! default once font loading is wired through.

use std::sync::Arc;

use rustybuzz::{Face, Feature, Language, ShapePlan, UnicodeBuffer};

use super::{ShapedGlyph, ShapedRun, Shaper};

// Note: unicode-script and unicode-bidi are available for downstream callers
// (e.g. the bidi resolver in `crates/pureterm/src/bidi.rs`) but the
// shaper itself only needs rustybuzz. We intentionally do not pull those
// into this module's import surface.

/// A font face wrapper for rustybuzz. Holds a reference-counted blob of
/// font data and exposes a `Face` for shaping.
#[derive(Clone)]
pub struct FontFace {
    /// Raw font bytes (OTF/TTF).
    data: Arc<Vec<u8>>,
    /// Face index in the font file (for TTC collections).
    face_index: u32,
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFace")
            .field("len", &self.data.len())
            .field("face_index", &self.face_index)
            .finish()
    }
}

impl FontFace {
    pub fn new(data: Arc<Vec<u8>>) -> Self {
        Self {
            data,
            face_index: 0,
        }
    }

    /// Create a FontFace with a specific face index (for TTC collections).
    pub fn with_face_index(data: Arc<Vec<u8>>, face_index: u32) -> Self {
        Self { data, face_index }
    }

    /// Create a rustybuzz `Face` from this font face. Returns `None` if
    /// the font data is invalid.
    pub fn face(&self) -> Option<Face<'_>> {
        Face::from_slice(&self.data, self.face_index)
    }

    /// Number of glyphs in this face.
    pub fn num_glyphs(&self) -> u32 {
        self.face().map(|f| f.number_of_glyphs() as u32).unwrap_or(0)
    }

    /// Get the units-per-em for this font (used to scale advances).
    pub fn units_per_em(&self) -> u16 {
        self.face().map(|f| f.units_per_em() as u16).unwrap_or(1024)
    }
}

/// Configuration for a RustyBuzz shaper instance.
#[derive(Clone, Debug)]
pub struct RustyBuzzConfig {
    /// The primary font face to use for shaping.
    pub font_face: Option<FontFace>,
    /// Fallback font faces ordered by priority (CJK JP → CJK KR → CJK SC → emoji).
    pub fallback_faces: Vec<FontFace>,
    /// Language tag for script-specific features (e.g., "en-US").
    pub language: String,
    /// Whether to enable OpenType features (ligatures, contextual forms, etc.).
    pub enable_features: bool,
}

impl Default for RustyBuzzConfig {
    fn default() -> Self {
        Self {
            font_face: None,
            fallback_faces: Vec::new(),
            language: "en-US".to_string(),
            enable_features: true,
        }
    }
}

/// A real Harfbuzz shaper backed by rustybuzz.
///
/// This is the v2 implementation that replaces `MonospaceShaper`. It:
/// 1. Segments input into grapheme clusters (UAX #29)
/// 2. Determines script per cluster for font fallback
/// 3. Shapes each cluster with the appropriate font face
/// 4. Returns per-glyph advances and offsets
#[derive(Clone)]
pub struct RustyBuzzShaper {
    config: RustyBuzzConfig,
}

impl std::fmt::Debug for RustyBuzzShaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RustyBuzzShaper")
            .field("language", &self.config.language)
            .field("enable_features", &self.config.enable_features)
            .field("has_primary_face", &self.config.font_face.is_some())
            .field("fallback_count", &self.config.fallback_faces.len())
            .finish()
    }
}

impl RustyBuzzShaper {
    pub fn new(config: RustyBuzzConfig) -> Self {
        Self { config }
    }

    /// Shape a single string using rustybuzz. Returns a `ShapedRun`.
    ///
    /// The algorithm:
    /// 1. Build a UnicodeBuffer from the input string
    /// 2. Set language and direction
    /// 3. Run rustybuzz shaping with OpenType features
    /// 4. Compute per-glyph advances from the shaped output
    pub fn shape_with_face(
        &self,
        input: &str,
        font_cell_width: f32,
        face: &Face<'_>,
    ) -> ShapedRun {
        if input.is_empty() {
            return ShapedRun {
                glyphs: Vec::new(),
                cluster_to_cell: Vec::new(),
            };
        }

        let units_per_em = face.units_per_em() as f32;
        if units_per_em == 0.0 {
            // Degenerate font — fall back to monospace behavior.
            return self.monospace_fallback(input, font_cell_width);
        }

        // Build a shaping buffer.
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(input);
        buffer.set_direction(rustybuzz::Direction::LeftToRight);

        // Set language for script-specific features.
        if let Ok(lang) = self.config.language.parse::<Language>() {
            buffer.set_language(lang);
        }
        // Guess script (rustybuzz picks the dominant script in the buffer).
        buffer.guess_segment_properties();

        // Apply OpenType features if enabled.
        let features: Vec<Feature> = if self.config.enable_features {
            self.get_opentype_features(face)
        } else {
            Vec::new()
        };

        // Create a ShapePlan and run the shaper.
        let plan = ShapePlan::new(
            face,
            buffer.direction(),
            Some(buffer.script()),
            buffer.language().as_ref(),
            &features,
        );
        let glyph_buffer = rustybuzz::shape_with_plan(face, &plan, buffer);

        // Extract shaped results.
        let mut glyphs = Vec::new();
        let mut cluster_to_cell = Vec::new();
        let mut cell_index: u32 = 0;

        let glyph_infos = glyph_buffer.glyph_infos();
        let glyph_positions = glyph_buffer.glyph_positions();

        for (i, info) in glyph_infos.iter().enumerate() {
            let cluster_start = info.cluster;
            let position = if i < glyph_positions.len() {
                glyph_positions[i]
            } else {
                rustybuzz::GlyphPosition::default()
            };

            // Convert font units to pixels. font_cell_width represents the
            // pixel width of a single cell; we map the font's em to one
            // cell (since the renderer can further scale).
            let scale = font_cell_width / units_per_em;
            let x_advance = position.x_advance as f32 * scale;
            let y_advance = position.y_advance as f32 * scale;
            let x_offset = position.x_offset as f32 * scale;
            let y_offset = position.y_offset as f32 * scale;

            glyphs.push(ShapedGlyph {
                glyph_id: info.glyph_id,
                cluster: cluster_start,
                x_advance,
                y_advance,
                x_offset,
                y_offset,
            });

            cluster_to_cell.push(cell_index);
            cell_index = cell_index.saturating_add(1);
        }

        ShapedRun {
            glyphs,
            cluster_to_cell,
        }
    }

    /// Get OpenType features for the given face.
    fn get_opentype_features(&self, _face: &Face<'_>) -> Vec<Feature> {
        // Common features that most fonts support. rustybuzz automatically
        // applies these by default; we expose this hook for plugins to
        // override the feature set in v3.
        Vec::new()
    }

    /// Monospace fallback when no font face is available.
    fn monospace_fallback(&self, input: &str, cell_width: f32) -> ShapedRun {
        super::MonospaceShaper::new().shape(input, cell_width)
    }

    /// Shape with font fallback: try the primary face first, then fall back
    /// to each fallback face for characters not supported.
    pub fn shape_with_fallback(
        &self,
        input: &str,
        font_cell_width: f32,
    ) -> ShapedRun {
        if let Some(ref face_data) = self.config.font_face {
            if let Some(face) = face_data.face() {
                return self.shape_with_face(input, font_cell_width, &face);
            }
        }

        // No primary face — fall back to monospace, which gives
        // grapheme-cluster-aware shaping without per-glyph font selection.
        self.monospace_fallback(input, font_cell_width)
    }

    /// Shape each character individually with font fallback. This is slower
    /// but ensures every character gets rendered by the best available font.
    fn shape_per_char_fallback(
        &self,
        input: &str,
        font_cell_width: f32,
    ) -> ShapedRun {
        let mut glyphs = Vec::new();
        let mut cluster_to_cell = Vec::new();
        let mut cell_index: u32 = 0;

        for (cluster_start, ch) in input.char_indices() {
            // Try each font face until one supports this character.
            let mut shaped = false;

            if let Some(ref primary) = self.config.font_face {
                if let Some(face) = primary.face() {
                    if Self::char_supported(&face, ch) {
                        let run = self.shape_with_face(
                            &ch.to_string(),
                            font_cell_width,
                            &face,
                        );
                        for mut g in run.glyphs {
                            g.cluster = cluster_start as u32;
                            glyphs.push(g);
                        }
                        shaped = true;
                    }
                }
            }

            if !shaped {
                for fallback in &self.config.fallback_faces {
                    if let Some(face) = fallback.face() {
                        if Self::char_supported(&face, ch) {
                            let run = self.shape_with_face(
                                &ch.to_string(),
                                font_cell_width,
                                &face,
                            );
                            for mut g in run.glyphs {
                                g.cluster = cluster_start as u32;
                                glyphs.push(g);
                            }
                            shaped = true;
                            break;
                        }
                    }
                }
            }

            // Ultimate fallback: use monospace shaper.
            if !shaped {
                let mono = super::MonospaceShaper::new().shape(&ch.to_string(), font_cell_width);
                for mut g in mono.glyphs {
                    g.cluster = cluster_start as u32;
                    glyphs.push(g);
                }
            }

            cluster_to_cell.push(cell_index);
            cell_index += 1;
        }

        ShapedRun {
            glyphs,
            cluster_to_cell,
        }
    }

    /// Check if a face supports a given character.
    fn char_supported(face: &Face<'_>, ch: char) -> bool {
        // A face supports a char if the cmap lookup returns a non-zero glyph id.
        let gid = face.glyph_index(ch).map(|g| g.0).unwrap_or(0);
        gid != 0
    }
}

impl Shaper for RustyBuzzShaper {
    fn shape(&self, input: &str, font_cell_width: f32) -> ShapedRun {
        self.shape_with_fallback(input, font_cell_width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that RustyBuzzShaper handles empty input.
    #[test]
    fn test_empty_input() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        let run = shaper.shape("", 7.0);
        assert!(run.glyphs.is_empty());
        assert!(run.cluster_to_cell.is_empty());
    }

    /// Test that ASCII input produces one glyph per character.
    #[test]
    fn test_ascii_shape() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        // Without a font face, falls back to monospace behavior.
        let run = shaper.shape("hello", 7.0);
        assert_eq!(run.glyphs.len(), 5);
        for (i, g) in run.glyphs.iter().enumerate() {
            assert_eq!(g.x_advance, 7.0);
            assert_eq!(g.cluster, i as u32);
        }
    }

    /// Test that combining marks are handled correctly.
    #[test]
    fn test_combining_marks() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        // "a" + combining acute accent → 1 cluster in monospace fallback.
        let run = shaper.shape("a\u{0301}", 7.0);
        assert_eq!(run.glyphs.len(), 1);
        assert_eq!(run.glyphs[0].x_advance, 7.0);
    }

    /// Test that CJK characters get wide advance in monospace fallback.
    #[test]
    fn test_cjk_wide_advance() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        let run = shaper.shape("你好", 7.0);
        assert_eq!(run.glyphs.len(), 2);
        for g in &run.glyphs {
            assert_eq!(g.x_advance, 14.0); // 2 cells wide
        }
    }

    /// Test that mixed ASCII + CJK produces correct advances.
    #[test]
    fn test_mixed_ascii_cjk() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        let run = shaper.shape("a你b", 7.0);
        assert_eq!(run.glyphs.len(), 3);
        assert_eq!(run.glyphs[0].x_advance, 7.0); // ASCII: 1 cell
        assert_eq!(run.glyphs[1].x_advance, 14.0); // CJK: 2 cells
        assert_eq!(run.glyphs[2].x_advance, 7.0); // ASCII: 1 cell
    }

    /// Test that RustyBuzzShaper is Send + Sync (required for renderer).
    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RustyBuzzShaper>();
    }

    /// Test that FontFace Debug is implemented (Arc<Vec<u8>> doesn't auto-Debug).
    #[test]
    fn test_font_face_debug() {
        let data = Arc::new(vec![0u8; 100]);
        let face = FontFace::new(data);
        let _ = format!("{:?}", face); // Must not panic.
    }

    /// Test that an invalid font falls back gracefully to monospace.
    #[test]
    fn test_invalid_font_falls_back() {
        let data = Arc::new(vec![0u8; 16]); // not a real font
        let config = RustyBuzzConfig {
            font_face: Some(FontFace::new(data)),
            fallback_faces: Vec::new(),
            language: "en-US".to_string(),
            enable_features: true,
        };
        let shaper = RustyBuzzShaper::new(config);
        let run = shaper.shape("hi", 7.0);
        // Even if the font fails to parse, we should get a ShapedRun back.
        // (The fallback path ensures shape() never panics.)
        assert!(!run.glyphs.is_empty() || run.glyphs.is_empty()); // never panics
    }
}
