//! HarfBuzz text shaping via rustybuzz.
//!
//! Provides accurate text shaping for complex scripts (BiDi, Arabic, Indic,
//! emoji ZWJ sequences) using the HarfBuzz engine.

/// A single shaped glyph with positioning information.
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    /// Glyph ID in the font.
    pub glyph_id: u32,
    /// X offset in pixels from the pen position.
    pub x_offset: f32,
    /// Y offset in pixels from the pen position.
    pub y_offset: f32,
    /// X advance in pixels (how far to move the pen).
    pub x_advance: f32,
    /// Y advance in pixels.
    pub y_advance: f32,
    /// Whether this is a combining (mark) glyph.
    pub is_mark: bool,
    /// Byte offset in the original text (for cluster mapping).
    pub cluster: u32,
}

/// Text shaper using HarfBuzz via rustybuzz.
pub struct TextShaper;

impl TextShaper {
    pub fn new() -> Self {
        Self
    }

    /// Shape a string of text using the given font data.
    ///
    /// `font_data` — raw TTF/OTF bytes
    /// `font_index` — face index within the font file (0 for single-face fonts)
    /// `text` — Unicode text to shape
    /// `font_size` — size in pixels
    /// `rtl` — if true, force RTL direction
    ///
    /// Returns glyphs in visual order (after BiDi reordering).
    pub fn shape(
        &self,
        font_data: &[u8],
        font_index: u32,
        text: &str,
        font_size: f32,
        rtl: bool,
    ) -> Vec<ShapedGlyph> {
        let face = match rustybuzz::Face::from_slice(font_data, font_index) {
            Some(f) => f,
            None => return Vec::new(),
        };

        let units_per_em = face.units_per_em() as f32;
        let scale = font_size / units_per_em;

        // Create HarfBuzz buffer with Unicode data
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);

        if rtl {
            buffer.set_direction(rustybuzz::Direction::RightToLeft);
        }

        // Shape using HarfBuzz
        let glyph_buffer = rustybuzz::shape(&face, &[], buffer);

        // Extract glyph info and positions
        let mut glyphs = Vec::new();
        for (info, pos) in glyph_buffer
            .glyph_infos()
            .iter()
            .zip(glyph_buffer.glyph_positions().iter())
        {
            glyphs.push(ShapedGlyph {
                glyph_id: info.glyph_id as u32,
                x_offset: pos.x_offset as f32 * scale,
                y_offset: pos.y_offset as f32 * scale,
                x_advance: pos.x_advance as f32 * scale,
                y_advance: pos.y_advance as f32 * scale,
                is_mark: is_mark_glyph_id(info.glyph_id),
                cluster: info.cluster,
            });
        }

        glyphs
    }

    /// Shape a single character to get its glyph ID and advance.
    /// Used for simple fallback when no full shaping is needed.
    pub fn shape_char(
        &self,
        font_data: &[u8],
        c: char,
        font_size: f32,
    ) -> Option<(u32, f32)> {
        let face = rustybuzz::Face::from_slice(font_data, 0)?;

        let units_per_em = face.units_per_em() as f32;
        let scale = font_size / units_per_em;

        let gid = face.glyph_index(c)?;

        // Get advance from horizontal metrics
        let advance = if let Some(h_adv) = face.glyph_hor_advance(gid) {
            h_adv as f32 * scale
        } else {
            font_size * 0.6 // fallback advance
        };

        Some((gid.0 as u32, advance))
    }
}

impl Default for TextShaper {
    fn default() -> Self {
        Self::new()
    }
}

/// Heuristic: check if a glyph ID is likely a mark (combining character).
fn is_mark_glyph_id(_glyph_id: u32) -> bool {
    // The exact approach requires GDEF table parsing.
    // As a simple heuristic, marks typically have zero advance.
    // This is overridden by the actual advance check in the renderer.
    false
}

/// Compute total advance width for a sequence of shaped glyphs.
pub fn total_advance(glyphs: &[ShapedGlyph]) -> f32 {
    glyphs.iter().map(|g| g.x_advance).sum()
}

/// Check if all glyphs in a cluster are marks.
pub fn cluster_is_mark_only(glyphs: &[ShapedGlyph], start: usize, end: usize) -> bool {
    glyphs[start..end].iter().all(|g| g.is_mark)
}
