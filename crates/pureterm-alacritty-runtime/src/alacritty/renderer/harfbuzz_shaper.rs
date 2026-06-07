//! HarfBuzz text shaping wrapper.
//!
//! Provides glyph-level text shaping with proper positioning for complex scripts
//! (Arabic, Indic, emoji ligatures, etc.). The `HarfBuzzShaper` takes a string
//! and font information, returns shaped glyphs with x/y offsets for correct
//! positioning.
//!
//! Architecture:
//! - Uses fontdb to locate font file data for HarfBuzz shaping
//! - Uses crossfont for rasterization (separate from shaping)
//! - HarfBuzz is only for computing glyph positions and substitutions

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use alacritty_terminal::index::Point;
use fontdb::Database;

use harfbuzz_sys::{
    hb_blob_create, hb_blob_destroy, hb_blob_get_empty,
    hb_blob_make_immutable, hb_blob_reference, hb_blob_set_user_data, hb_blob_get_user_data,
    hb_blob_copy_writable_or_fail, hb_blob_create_sub_blob, hb_blob_get_data, hb_blob_get_length,
};
use harfbuzz_sys::{
    hb_face_count, hb_face_create, hb_face_create_for_tables, hb_face_destroy, hb_face_get_empty,
    hb_face_get_index, hb_face_get_upem, hb_face_is_immutable, hb_face_make_immutable,
    hb_face_reference, hb_face_reference_blob, hb_face_reference_table, hb_face_set_index,
    hb_face_set_upem, hb_face_set_user_data, hb_face_get_user_data, hb_font_create,
    hb_font_create_sub_font, hb_font_destroy, hb_font_get_empty, hb_font_get_face,
    hb_font_get_parent, hb_font_get_scale, hb_font_get_user_data, hb_font_is_immutable,
    hb_font_reference, hb_font_set_face, hb_font_set_parent, hb_font_set_scale,
    hb_font_set_user_data,
};
use harfbuzz_sys::{
    hb_buffer_reset, hb_buffer_create, hb_buffer_get_empty, hb_buffer_reference,
    hb_buffer_destroy, hb_buffer_set_user_data, hb_buffer_get_user_data, hb_buffer_set_content_type,
    hb_buffer_get_content_type, hb_buffer_set_unicode_funcs, hb_buffer_get_unicode_funcs,
    hb_buffer_set_direction, hb_buffer_get_direction, hb_buffer_set_script, hb_buffer_get_script,
    hb_buffer_set_language, hb_buffer_get_language, hb_buffer_guess_segment_properties,
    hb_buffer_add_utf8, hb_buffer_add_utf16, hb_buffer_add_utf32, hb_buffer_add_latin1,
    hb_buffer_add_codepoints, hb_buffer_get_length, hb_buffer_get_glyph_infos,
    hb_buffer_get_glyph_positions, hb_buffer_clear_contents, hb_buffer_pre_allocate,
    hb_buffer_allocation_successful, hb_buffer_reverse, hb_buffer_reverse_clusters,
    hb_buffer_set_cluster_level, hb_buffer_get_cluster_level, hb_buffer_set_invisible_glyph,
    hb_buffer_get_invisible_glyph, hb_buffer_set_replacement_codepoint,
    hb_buffer_get_replacement_codepoint,
};
use harfbuzz_sys::{
    hb_shape, hb_shape_full, hb_shape_plan_create, hb_shape_plan_create_cached,
    hb_shape_plan_create_cached2, hb_shape_plan_destroy, hb_shape_plan_execute,
    hb_shape_plan_get_shaper, hb_shape_plan_get_empty,
};
use harfbuzz_sys::{
    hb_glyph_info_t, hb_glyph_position_t, hb_codepoint_t, hb_position_t, hb_mask_t, hb_bool_t,
    hb_tag_t, hb_var_int_t, hb_direction_t, hb_script_t, hb_language_t, hb_buffer_content_type_t,
    hb_buffer_flags_t, hb_buffer_cluster_level_t, hb_memory_mode_t,
};
use harfbuzz_sys::{
    HB_MEMORY_MODE_READONLY, HB_DIRECTION_LTR, HB_SCRIPT_LATIN,
};

// ── Types ────────────────────────────────────────────────────────────────────

/// Result of shaping a single glyph within a text run.
#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    /// HarfBuzz glyph index within the shaped font.
    pub glyph_index: u32,
    /// Horizontal offset (pixels) from the glyph's natural advance.
    pub x_offset: f32,
    /// Vertical offset (pixels) from the baseline.
    pub y_offset: f32,
    /// Horizontal advance (pixels).
    pub x_advance: f32,
    /// Cluster index into the original codepoint string.
    pub cluster: u32,
    /// Whether this glyph marks the end of a cluster.
    pub is_cluster_end: bool,
}

/// A shaped text run.
///
/// `text` carries the original codepoints. PureTerm uses `Cow<'a, str>` instead
/// of `String` so the common borrowed-input path skips a heap allocation per run.
#[derive(Clone, Debug)]
pub struct ShapedRun<'a> {
    /// The codepoints that were shaped.
    pub text: Cow<'a, str>,
    /// Shaped glyphs in visual order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Font index in the atlas.
    pub font_index: usize,
}

/// Cache for shaping results indexed by Point.
///
/// Used to store per-cell x/y offsets from HarfBuzz shaping so they can be
/// applied during vertex building.
#[derive(Default, Clone, Debug)]
pub struct ShapedGlyphCache {
    /// Shaping offsets indexed by linear cell position.
    offsets: HashMap<usize, (f32, f32)>,
    /// Number of columns (for computing linear index).
    cols: usize,
}

impl ShapedGlyphCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of columns for index computation.
    pub fn set_columns(&mut self, cols: usize) {
        self.cols = cols;
    }

    /// Store shaping offset for a cell at point.
    pub fn set_offset_for_point(&mut self, point: &Point<usize>, x_offset: f32, y_offset: f32) {
        let idx = point.line * self.cols.max(1) + point.column.0;
        self.offsets.insert(idx, (x_offset, y_offset));
    }

    /// Get shaping offset for a cell at point. Returns (0, 0) if not shaped.
    pub fn get_offset_for_point(&self, point: &Point<usize>) -> (f32, f32) {
        let idx = point.line * self.cols.max(1) + point.column.0;
        self.offsets.get(&idx).copied().unwrap_or((0.0, 0.0))
    }

    /// Clear all cached shaping data.
    pub fn clear(&mut self) {
        self.offsets.clear();
    }
}

// ── HarfBuzzShaper ────────────────────────────────────────────────────────────

/// HarfBuzz text shaper.
///
/// Shapes text runs using HarfBuzz, returning glyph positions for correct
/// rendering of complex scripts and ligatures.
pub struct HarfBuzzShaper {
    /// Cached font face data per fontdb face ID, wrapped in `Arc` so the
    /// bytes can outlive a single `shape()` call when handed to HarfBuzz
    /// (`HB_MEMORY_MODE_READONLY` borrows the pointer, no copy).
    face_data: HashMap<fontdb::ID, Arc<Vec<u8>>>,
    /// Parsed `ttf_parser::Face` per fontdb face ID. Skips the parse step
    /// in `face_has_codepoint` on cache hit (the hot path during font
    /// fallback resolution).
    ///
    /// SAFETY invariant: each `Face<'static>` here borrows from the
    /// corresponding `Arc<Vec<u8>>` in `face_data`. The lifetime is
    /// extended to `'static` via `mem::transmute`; the invariant is
    /// upheld because (a) `face_data` is never cleared partially (only
    /// via `clear_cache` which clears both maps), and (b) `Arc` keeps
    /// the bytes alive as long as any `Face` exists in this map.
    parsed_faces: HashMap<fontdb::ID, ttf_parser::Face<'static>>,
    /// fontdb for font file lookups.
    fontdb: Database,
}

impl HarfBuzzShaper {
    /// Create a new HarfBuzz shaper.
    pub fn new() -> Self {
        let mut fontdb = Database::new();
        fontdb.load_system_fonts();
        Self {
            face_data: HashMap::new(),
            parsed_faces: HashMap::new(),
            fontdb,
        }
    }

    /// Load font file data for a fontdb face. Returns `Arc<Vec<u8>>` so the
    /// caller can hand the bytes to HarfBuzz without copying.
    fn load_face_data(&mut self, face_id: fontdb::ID) -> Option<Arc<Vec<u8>>> {
        if let Some(data) = self.face_data.get(&face_id) {
            return Some(data.clone());
        }

        let (source, index) = self.fontdb.face_source(face_id)?;

        let data: Vec<u8> = match source {
            fontdb::Source::File(ref path) => std::fs::read(path).ok()?,
            fontdb::Source::Binary(ref bytes) => {
                let bytes_ref: &dyn AsRef<[u8]> = bytes.as_ref();
                bytes_ref.as_ref().to_vec()
            },
            fontdb::Source::SharedFile(ref path, _) => std::fs::read(path).ok()?,
        };

        let arc = Arc::new(data);

        // Parse the ttf_parser face while the Arc is local. We then store
        // both: `arc` in `face_data`, the `Face` borrowing from it in
        // `parsed_faces`. The `Face` lifetime is extended to `'static` via
        // `mem::transmute`; this is sound because the Arc keeps the bytes
        // alive as long as the Face is reachable through this map.
        let parsed: ttf_parser::Face<'_> = ttf_parser::Face::parse(&arc, index).ok()?;
        // SAFETY: see the field-level doc comment on `parsed_faces`.
        let parsed_static: ttf_parser::Face<'static> = unsafe { std::mem::transmute(parsed) };

        self.face_data.insert(face_id, arc.clone());
        self.parsed_faces.insert(face_id, parsed_static);
        Some(arc)
    }

    /// Find the fontdb face ID for a given codepoint.
    fn find_face_for_codepoint(&mut self, cp: u32, families: &[&str]) -> Option<fontdb::ID> {
        // Convert codepoint to char for lookup.
        let c = char::from_u32(cp).unwrap_or('\u{FFFD}');

        // Collect face IDs matching the requested families first to avoid
        // borrowing `self.fontdb` while also calling `face_has_codepoint`
        // (which needs `&mut self`).
        let mut family_match_ids: Vec<fontdb::ID> = Vec::new();
        for family in families {
            for face_info in self.fontdb.faces() {
                for (fam, _) in &face_info.families {
                    if fam == *family {
                        family_match_ids.push(face_info.id);
                    }
                }
            }
        }
        for id in &family_match_ids {
            if self.face_has_codepoint(*id, c) {
                return Some(*id);
            }
        }

        // Generic fallback - any face that has this codepoint.
        let all_ids: Vec<fontdb::ID> = self.fontdb.faces().map(|f| f.id).collect();
        for id in &all_ids {
            if self.face_has_codepoint(*id, c) {
                return Some(*id);
            }
        }

        None
    }

    /// Check if a fontdb face supports a codepoint.
    fn face_has_codepoint(&mut self, face_id: fontdb::ID, c: char) -> bool {
        // PureTerm: use the cached `parsed_faces` to avoid re-parsing the
        // font file on every codepoint lookup (called hundreds of times
        // per `shape()` while resolving font fallback).
        if let Some(face) = self.parsed_faces.get(&face_id) {
            return face.glyph_index(c).is_some();
        }
        // Cache miss: populate via `load_face_data` which parses + caches.
        if self.load_face_data(face_id).is_none() {
            return false;
        }
        self.parsed_faces
            .get(&face_id)
            .and_then(|face| face.glyph_index(c))
            .is_some()
    }

    /// Shape a text string with HarfBuzz.
    ///
    /// Returns shaped glyphs with positions. The caller is responsible for
    /// rasterizing each glyph through crossfont.
    ///
    /// The `families` slice provides font family names to try, in order.
    /// On macOS, the first match wins (e.g. "PingFang SC" for CJK).
    pub fn shape<'a>(
        &mut self,
        text: &'a str,
        font_index: usize,
        families: &[&str],
    ) -> ShapedRun<'a> {
        // Find a suitable face for this text.
        let face_id = text.chars().next().and_then(|c| {
            self.find_face_for_codepoint(c as u32, families)
        });

        let Some(face_id) = face_id else {
            // No suitable font found - fallback to no shaping.
            // PureTerm: 预分配 text.len()（UTF-8 字节上界即 char 上界）。
            let mut glyphs: Vec<ShapedGlyph> = Vec::with_capacity(text.len());
            glyphs.extend(text.chars().enumerate().map(|(i, c)| ShapedGlyph {
                glyph_index: c as u32,
                x_offset: 0.0,
                y_offset: 0.0,
                x_advance: 0.0,
                cluster: i as u32,
                is_cluster_end: true,
            }));
            return ShapedRun {
                text: Cow::Borrowed(text),
                glyphs,
                font_index,
            };
        };

        // Load font data and create HarfBuzz objects.
        let Some(font_data) = self.load_face_data(face_id) else {
            return ShapedRun {
                text: Cow::Borrowed(text),
                glyphs: Vec::new(),
                font_index,
            };
        };

        // Create blob. `font_data` is `Arc<Vec<u8>>` — the bytes stay alive
        // for the lifetime of `blob` because we hold the Arc locally.
        let blob = unsafe {
            hb_blob_create(
                font_data.as_ptr() as *const std::ffi::c_char,
                font_data.len() as u32,
                HB_MEMORY_MODE_READONLY,
                std::ptr::null_mut(),
                None,
            )
        };

        if blob.is_null() {
            return ShapedRun {
                text: Cow::Borrowed(text),
                glyphs: Vec::new(),
                font_index,
            };
        }

        // Create face.
        let face = unsafe { hb_face_create(blob, 0) };

        if face.is_null() {
            unsafe { hb_blob_destroy(blob); }
            return ShapedRun {
                text: Cow::Borrowed(text),
                glyphs: Vec::new(),
                font_index,
            };
        }

        // Create font.
        let font = unsafe { hb_font_create(face) };

        if font.is_null() {
            unsafe {
                hb_face_destroy(face);
                hb_blob_destroy(blob);
            }
            return ShapedRun {
                text: Cow::Borrowed(text),
                glyphs: Vec::new(),
                font_index,
            };
        }

        // Set scale to 26.6 fixed point (64 units per pixel).
        // For a typical 16px font, scale = 16 * 64 = 1024.
        let scale = 1024i32;
        unsafe {
            hb_font_set_scale(font, scale, scale);
        }

        // Create buffer and add UTF-8 text.
        let buffer = unsafe { hb_buffer_create() };

        if buffer.is_null() {
            unsafe {
                hb_font_destroy(font);
                hb_face_destroy(face);
                hb_blob_destroy(blob);
            }
            return ShapedRun {
                text: Cow::Borrowed(text),
                glyphs: Vec::new(),
                font_index,
            };
        }

        unsafe {
            hb_buffer_reset(buffer);
            hb_buffer_guess_segment_properties(buffer);
        }

        // Add text as UTF-8.
        let bytes = text.as_bytes();
        unsafe {
            hb_buffer_add_utf8(
                buffer,
                bytes.as_ptr() as *const i8,
                bytes.len() as i32,
                0,
                bytes.len() as i32,
            );
        }

        // Shape the buffer.
        unsafe {
            hb_shape(font, buffer, std::ptr::null(), 0);
        }

        // Extract glyph count.
        let glyph_count = unsafe { hb_buffer_get_length(buffer) } as usize;

        if glyph_count == 0 {
            unsafe {
                hb_buffer_destroy(buffer);
                hb_font_destroy(font);
                hb_face_destroy(face);
                hb_blob_destroy(blob);
            }
            return ShapedRun {
                text: Cow::Borrowed(text),
                glyphs: Vec::new(),
                font_index,
            };
        }

        // Get glyph info and positions via returned pointers.
        let mut glyph_count = glyph_count as u32;
        let glyph_infos_ptr = unsafe {
            hb_buffer_get_glyph_infos(buffer, &mut glyph_count)
        };
        let mut pos_count = glyph_count;
        let glyph_positions_ptr = unsafe {
            hb_buffer_get_glyph_positions(buffer, &mut pos_count)
        };

        if glyph_count == 0 || glyph_infos_ptr.is_null() {
            unsafe {
                hb_buffer_destroy(buffer);
                hb_font_destroy(font);
                hb_face_destroy(face);
                hb_blob_destroy(blob);
            }
            return ShapedRun {
                text: Cow::Borrowed(text),
                glyphs: Vec::new(),
                font_index,
            };
        }

        // Build ShapedGlyph list from HarfBuzz output.
        let glyph_count = glyph_count as usize;
        // PureTerm: 预分配 glyph_count 容量，避免 Vec 在 collect 中扩容。
        let mut glyphs: Vec<ShapedGlyph> = Vec::with_capacity(glyph_count);
        glyphs.extend((0..glyph_count).map(|i| {
            let info = unsafe { &*glyph_infos_ptr.add(i) };
            let pos = if glyph_positions_ptr.is_null() {
        &hb_glyph_position_t {
                    x_advance: 0,
                    y_advance: 0,
                    x_offset: 0,
                    y_offset: 0,
                    var: hb_var_int_t { u32: 0 },
                }
            } else {
                unsafe { &*glyph_positions_ptr.add(i) }
            };

            let is_cluster_end = if i + 1 < glyph_count {
                let next_info = unsafe { &*glyph_infos_ptr.add(i + 1) };
                next_info.cluster != info.cluster
            } else {
                true
            };

            ShapedGlyph {
                glyph_index: info.codepoint,
                x_offset: pos.x_offset as f32 / 64.0,
                y_offset: pos.y_offset as f32 / 64.0,
                x_advance: pos.x_advance as f32 / 64.0,
                cluster: info.cluster,
                is_cluster_end,
            }
        }));

        // Cleanup.
        unsafe {
            hb_buffer_destroy(buffer);
            hb_font_destroy(font);
            hb_face_destroy(face);
            hb_blob_destroy(blob);
        }

        ShapedRun {
            text: Cow::Borrowed(text),
            glyphs,
            font_index,
        }
    }

    /// Clear the font data cache.
    ///
    /// Call this when font configuration changes.
    pub fn clear_cache(&mut self) {
        self.face_data.clear();
        self.parsed_faces.clear();
    }
}

impl Default for HarfBuzzShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HarfBuzzShaper {
    fn drop(&mut self) {
        self.face_data.clear();
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shaper_creation() {
        let shaper = HarfBuzzShaper::new();
        assert!(shaper.face_data.is_empty());
    }
}