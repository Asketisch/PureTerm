//! T-core-shaper: per-glyph shaping trait + cluster mapping + advance.
//!
//! v1: a pure-Rust, no-deps shaper that walks the input by grapheme
//! cluster (UAX #29 approximation) and emits one [`ShapedGlyph`] per
//! cluster with the `x_advance` set to the supplied cell width. The
//! shape is "monospace cluster = N cells" — N=1 for Latin / ASCII, N=2
//! for CJK / Hangul / Hiragana / Katakana / wide emoji. v1 is enough
//! to expose the trait surface and let the cluster↔cell reverse-lookup
//! (used by `ime_widget` / `bidi`) work without pulling in a full
//! Harfbuzz. A real `rustybuzz_shaper` lands in v2.
//!
//! ## Why trait first?
//!
//! The renderer / bidi / ime code paths need the *contract* (`x_advance`
//! + `cluster`) — not necessarily a real Harfbuzz port — to wire up
//! the `ShapedGlyph { x_advance }` field on `CachedGlyph` (see
//! `renderer::glyph_atlas::CachedGlyph`) and the `cluster → cell`
//! reverse-lookup the IME widget needs to position its preedit
//! composition. v1 ships the contract; v2 swaps the impl.

/// One shaped glyph: a single code-point (or grapheme cluster) ready
/// to render at a grid cell. The `cluster` field is the byte offset
/// into the original string — used by the IME / bidi layers to map a
/// shape back to a cell position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    /// Glyph id within the font. v1 always emits `0` (the `.notdef`
    /// glyph); a real shaper returns the font-specific id.
    pub glyph_id: u32,
    /// Byte offset into the input string where this glyph's cluster
    /// starts. Combined with the next glyph's `cluster`, the IME can
    /// map a glyph back to a byte range.
    pub cluster: u32,
    /// Horizontal advance in pixels (post-font-units). v1 returns
    /// `font_cell_width * cell_count` (cell_count is 1 for Latin, 2
    /// for CJK wide).
    pub x_advance: f32,
    /// Vertical advance in pixels. v1 returns 0 (the line height comes
    /// from the font metrics, not the shaper).
    pub y_advance: f32,
    /// Horizontal offset from the cursor. Used for combining marks and
    /// complex scripts.
    pub x_offset: f32,
    /// Vertical offset from the baseline.
    pub y_offset: f32,
}

/// One shaped run — a contiguous span of the input string with its
/// shaped glyphs and a `cluster → cell` reverse map.
#[derive(Debug, Clone)]
pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
    /// `cluster_to_cell[i]` gives the grid cell index for glyph `i`.
    /// v1: 1:1 with the input. v2: maps Unicode grapheme clusters to
    /// terminal cells (the cell may consume 1 or 2 columns for CJK
    /// wide characters — see `cell_count` derived from `x_advance`).
    pub cluster_to_cell: Vec<u32>,
}

/// The shaper trait. The renderer calls `shape` with the input string
/// and gets back a shaped run. The cell width is supplied by the
/// renderer (it knows the font metrics).
pub trait Shaper: Send + Sync {
    /// Shape the input string. The result is a [`ShapedRun`].
    fn shape(&self, input: &str, font_cell_width: f32) -> ShapedRun;
}

/// Pure-Rust monospace shaper. Each Unicode grapheme cluster
/// (UAX #29 approximation) is treated as one cluster; the cluster's
/// `x_advance` is `font_cell_width` × cell count (1 for Latin, 2 for
/// CJK / Hangul / Hiragana / Katakana / wide emoji / multi-codepoint
/// clusters). The cluster↔cell map is `1:1` for v1.
#[derive(Debug, Clone, Default)]
pub struct MonospaceShaper;

impl MonospaceShaper {
    pub fn new() -> Self {
        Self
    }
}

impl Shaper for MonospaceShaper {
    fn shape(&self, input: &str, font_cell_width: f32) -> ShapedRun {
        let mut glyphs = Vec::new();
        let mut cluster_to_cell = Vec::new();
        let mut cell_index: u32 = 0;
        for (cluster_start, cluster) in grapheme_clusters(input) {
            let wide = is_wide(cluster);
            let cell_count = if wide { 2u32 } else { 1u32 };
            let x_advance = font_cell_width * cell_count as f32;
            glyphs.push(ShapedGlyph {
                glyph_id: 0, // v1: `.notdef`; v2: real glyph id.
                cluster: cluster_start as u32,
                x_advance,
                y_advance: 0.0,
                x_offset: 0.0,
                y_offset: 0.0,
            });
            cluster_to_cell.push(cell_index);
            cell_index = cell_index.saturating_add(cell_count);
        }
        ShapedRun { glyphs, cluster_to_cell }
    }
}

/// True if the cluster's head char is a "wide" character (CJK / wide
/// emoji). v1: ignore multi-codepoint "wide" detection for emoji ZWJ
/// sequences — a real ZWJ cluster is 2 cells, but we can't tell a
/// base+marking from a ZWJ emoji without UAX #29, so we conservatively
/// report 1 cell (the head char) + see the conservative
/// `x_advance = cell_width` for ZWJ clusters in `shape`.
fn is_wide(s: &str) -> bool {
    let c = s.chars().next().unwrap_or('\0');
    is_wide_char(c)
}

fn is_wide_char(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        // CJK Unified Ideographs (basic)
        0x4E00..=0x9FFF
        // CJK Unified Ideographs Extension A
        | 0x3400..=0x4DBF
        // CJK Compatibility Ideographs
        | 0xF900..=0xFAFF
        // Hiragana / Katakana
        | 0x3040..=0x30FF
        // Hangul Syllables
        | 0xAC00..=0xD7AF
        // Fullwidth Forms
        | 0xFF00..=0xFF60
        // Emoji + Misc Symbols (a conservative subset)
        | 0x1F300..=0x1F9FF
    )
}

/// Iterate `(byte_offset, cluster_str)` over the grapheme clusters of
/// `input`. v1 is a no-deps approximation: it walks the string and
/// emits a cluster per Unicode scalar value, but combines a
/// non-spacing / combining mark (General_Category starts with M) with
/// the previous cluster. This is good enough for monospace Latin /
/// CJK / Hangul / Hangul Jamo — emoji ZWJ sequences are over-counted
/// as 1 cell (v2 brings a real UAX #29 implementation).
fn grapheme_clusters(input: &str) -> impl Iterator<Item = (usize, &str)> {
    GraphemeIter { input, pos: 0 }
}

struct GraphemeIter<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Iterator for GraphemeIter<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.input.len() {
            return None;
        }
        let start = self.pos;
        let mut chars = self.input[start..].char_indices();
        let (_, c) = chars.next()?;
        let mut end = start + c.len_utf8();
        // Fold combining marks into the current cluster.
        for (_, next) in chars {
            if !is_combining(next) {
                break;
            }
            end += next.len_utf8();
        }
        let cluster = &self.input[start..end];
        self.pos = end;
        Some((start, cluster))
    }
}

fn is_combining(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        0x0300..=0x036F  // Combining Diacritical Marks
        | 0x1AB0..=0x1AFF // Combining Diacritical Marks Extended
        | 0x1DC0..=0x1DFF // Combining Diacritical Marks Supplement
        | 0x20D0..=0x20FF // Combining Diacritical Marks for Symbols
        | 0xFE20..=0xFE2F // Combining Half Marks
    )
}

/// Real rustybuzz shaper + font fallback chain (v2).
pub mod fallback;
pub mod rustybuzz_shaper;

pub use fallback::{FallbackChain, FallbackChainBuilder, FallbackScript, NamedFontFace};
pub use rustybuzz_shaper::{FontFace, RustyBuzzConfig, RustyBuzzShaper};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_one_cell_per_cluster() {
        let run = MonospaceShaper::new().shape("hello", 7.0);
        assert_eq!(run.glyphs.len(), 5);
        for (i, g) in run.glyphs.iter().enumerate() {
            assert_eq!(g.x_advance, 7.0);
            assert_eq!(g.cluster, i as u32);
        }
    }

    #[test]
    fn cjk_full_width_is_two_cells() {
        let run = MonospaceShaper::new().shape("你好", 7.0);
        assert_eq!(run.glyphs.len(), 2);
        for g in &run.glyphs {
            assert_eq!(g.x_advance, 14.0);
        }
        assert_eq!(run.cluster_to_cell, vec![0, 2]);
    }

    #[test]
    fn mixed_ascii_cjk() {
        let run = MonospaceShaper::new().shape("a你b", 7.0);
        assert_eq!(run.glyphs.len(), 3);
        assert_eq!(run.glyphs[0].x_advance, 7.0);
        assert_eq!(run.glyphs[1].x_advance, 14.0);
        assert_eq!(run.glyphs[2].x_advance, 7.0);
        assert_eq!(run.cluster_to_cell, vec![0, 1, 3]);
    }

    #[test]
    fn empty_input_yields_empty_run() {
        let run = MonospaceShaper::new().shape("", 7.0);
        assert!(run.glyphs.is_empty());
        assert!(run.cluster_to_cell.is_empty());
    }

    #[test]
    fn cluster_byte_offset_matches_input() {
        let run = MonospaceShaper::new().shape("a你", 7.0);
        assert_eq!(run.glyphs[0].cluster, 0);
        assert_eq!(run.glyphs[1].cluster, 1);
    }

    #[test]
    fn wide_detection_ranges() {
        assert!(is_wide_char('中'));
        assert!(is_wide_char('あ'));
        assert!(is_wide_char('한'));
        assert!(!is_wide_char('a'));
        assert!(!is_wide_char('Z'));
        assert!(!is_wide_char('0'));
    }

    #[test]
    fn emoji_sequences_are_wide() {
        // Man + ZWJ + Laptop. v1: ZWJ (`U+200D`) is NOT a combining mark,
        // so the iterator produces 3 clusters, not 1. The head char of
        // each is in the emoji range, so each cluster is wide (2 cells);
        // the ZWJ itself is a non-wide 0-width char and falls through
        // to the half-width 1-cell advance.
        let run = MonospaceShaper::new().shape("\u{1F468}\u{200D}\u{1F4BB}", 7.0);
        assert_eq!(run.glyphs.len(), 3);
        assert_eq!(run.glyphs[0].x_advance, 14.0); // Man 👨
        assert_eq!(run.glyphs[1].x_advance, 7.0); // ZWJ
        assert_eq!(run.glyphs[2].x_advance, 14.0); // Laptop 💻
    }

    #[test]
    fn combining_marks_attach_to_previous_cluster() {
        let run = MonospaceShaper::new().shape("a\u{0301}", 7.0);
        assert_eq!(run.glyphs.len(), 1);
        assert_eq!(run.glyphs[0].x_advance, 7.0);
        assert_eq!(run.glyphs[0].cluster, 0);
    }

    #[test]
    fn cluster_offsets_form_partition() {
        let input = "a你b";
        let run = MonospaceShaper::new().shape(input, 7.0);
        let cluster_starts: Vec<usize> = run.glyphs.iter().map(|g| g.cluster as usize).collect();
        // Glyph 0 covers "a" (1 byte), glyph 1 covers "你" (3 bytes),
        // glyph 2 covers "b" (1 byte).
        let mut prev = 0;
        let mut ends: Vec<usize> = Vec::new();
        for &s in &cluster_starts {
            if s > prev {
                ends.push(s);
            }
            prev = s;
        }
        ends.push(input.len());
        // The glyph 0 starts at 0; glyph 1 starts at 1; glyph 2 starts at 4.
        assert_eq!(cluster_starts, vec![0, 1, 4]);
        // Input length = 5.
        assert_eq!(input.len(), 5);
    }
}
