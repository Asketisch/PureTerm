//! Bidirectional text support (UAX #9).
//!
//! Provides per-line visual-to-logical mapping for RTL text rendering using
//! the `unicode_bidi` crate. Terminal lines are always stored in logical order;
//! this module computes the reordering needed for correct visual display.

use crate::screen::cell::WideMode;
use crate::screen::Cell;
use unicode_bidi::BidiInfo;

/// Per-line bidirectional mapping.
///
/// Each terminal line is stored in logical order. When the line contains RTL
/// text, `visual_to_logical` maps visual column indices to logical column
/// indices so the renderer can display cells in the correct order.
#[derive(Debug, Clone)]
pub struct BidiLine {
    /// Maps visual column index to logical column index.
    ///
    /// For a pure LTR line this is the identity mapping `[0, 1, 2, ...]`.
    visual_to_logical: Vec<usize>,
    /// Whether this line contains any RTL text.
    has_rtl: bool,
}

impl BidiLine {
    /// Build a bidi mapping for a single row of cells.
    ///
    /// Extracts characters from non-spacer cells, runs UAX #9 analysis, and
    /// constructs the visual-to-logical mapping. Wide character spacer cells
    /// (`SpacerTail` / `SpacerHead`) are skipped in the bidi analysis but
    /// re-inserted into the mapping so they travel with their head cell.
    pub fn new(line: &[Cell]) -> Self {
        let len = line.len();

        if len == 0 {
            return Self {
                visual_to_logical: Vec::new(),
                has_rtl: false,
            };
        }

        // Collect characters from non-spacer cells and track their column indices.
        let mut chars = String::with_capacity(len);
        let mut col_to_char_idx: Vec<usize> = Vec::with_capacity(len);
        let mut char_idx_to_col: Vec<usize> = Vec::with_capacity(len);

        for (col, cell) in line.iter().enumerate() {
            match cell.wide {
                WideMode::SpacerTail | WideMode::SpacerHead => {
                    // Skip spacers in bidi analysis; they will be handled
                    // separately after reordering.
                }
                WideMode::Narrow | WideMode::Wide => {
                    let char_idx = chars.len();
                    chars.push(cell.c);
                    col_to_char_idx.push(char_idx);
                    char_idx_to_col.push(col);
                }
            }
        }

        let num_chars = chars.len();
        if num_chars == 0 {
            return Self {
                visual_to_logical: identity_mapping(len),
                has_rtl: false,
            };
        }

        // Run bidi analysis with auto-detection of paragraph level.
        let bidi_info = BidiInfo::new(&chars, None);

        if !bidi_info.has_rtl() {
            return Self {
                visual_to_logical: identity_mapping(len),
                has_rtl: false,
            };
        }

        // Get reordered levels for the first (and only) paragraph.
        let para = &bidi_info.paragraphs[0];
        let levels = bidi_info.reordered_levels_per_char(para, para.range.clone());

        // Reorder character-level indices using the resolved levels.
        let char_reorder = BidiInfo::reorder_visual(&levels);

        // Build visual_to_logical mapping at the cell-column level.
        //
        // Strategy: iterate through the reordered character positions, and for
        // each character's column, also include any spacer cell that belongs to
        // it (SpacerTail after a Wide cell, or SpacerHead before a wrapped
        // wide cell).
        let mut visual_to_logical = Vec::with_capacity(len);
        let mut used = vec![false; len];

        for &char_pos in &char_reorder {
            let col = char_idx_to_col[char_pos];
            // If this is a Wide character, first check if there is a SpacerHead
            // before it (wrapped from previous line — should not occur mid-line
            // but handle defensively).
            if line[col].wide == WideMode::Wide {
                if col + 1 < len && line[col + 1].wide == WideMode::SpacerTail {
                    if !used[col + 1] {
                        visual_to_logical.push(col + 1);
                        used[col + 1] = true;
                    }
                }
            }
            if !used[col] {
                visual_to_logical.push(col);
                used[col] = true;
            }
        }

        // Any remaining unused columns (stray spacers, etc.) get appended in
        // their original order at the end.
        for col in 0..len {
            if !used[col] {
                visual_to_logical.push(col);
            }
        }

        // Safety: the mapping should have exactly `len` entries.
        debug_assert_eq!(visual_to_logical.len(), len);

        Self {
            visual_to_logical,
            has_rtl: true,
        }
    }

    /// Get the visual column index for a given logical column.
    ///
    /// Returns the position at which the cell at `logical` should be rendered.
    /// For LTR-only lines this is always the identity.
    pub fn visual_col(&self, logical: usize) -> usize {
        if !self.has_rtl {
            return logical;
        }
        self.visual_to_logical
            .iter()
            .position(|&v| v == logical)
            .unwrap_or(logical)
    }

    /// Get the logical column index for a given visual column.
    ///
    /// Returns the cell column that should be displayed at visual position
    /// `visual`.
    pub fn logical_col(&self, visual: usize) -> usize {
        if !self.has_rtl {
            return visual;
        }
        if visual < self.visual_to_logical.len() {
            self.visual_to_logical[visual]
        } else {
            visual
        }
    }

    /// Whether this line contains any RTL text that requires reordering.
    pub fn has_rtl(&self) -> bool {
        self.has_rtl
    }

    /// Returns true if no reordering is needed (pure LTR or empty).
    pub fn is_identity(&self) -> bool {
        !self.has_rtl
    }

    /// Access the raw visual-to-logical mapping.
    pub fn as_slice(&self) -> &[usize] {
        &self.visual_to_logical
    }
}

/// Create an identity mapping `[0, 1, 2, ..., len-1]`.
fn identity_mapping(len: usize) -> Vec<usize> {
    (0..len).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::cell::WideMode;
    use crate::screen::Cell;

    fn cell_narrow(c: char) -> Cell {
        Cell::new(c)
    }

    fn cell_wide(c: char) -> Cell {
        let mut cell = Cell::new(c);
        cell.wide = WideMode::Wide;
        cell
    }

    fn cell_spacer_tail() -> Cell {
        let mut cell = Cell::new(' ');
        cell.wide = WideMode::SpacerTail;
        cell
    }

    #[test]
    fn empty_line() {
        let line: Vec<Cell> = vec![];
        let bidi = BidiLine::new(&line);
        assert!(!bidi.has_rtl());
        assert!(bidi.is_identity());
        assert!(bidi.visual_to_logical.is_empty());
    }

    #[test]
    fn pure_ltr_line() {
        let line: Vec<Cell> = "Hello".chars().map(cell_narrow).collect();
        let bidi = BidiLine::new(&line);
        assert!(!bidi.has_rtl());
        assert!(bidi.is_identity());
        // Identity mapping
        for i in 0..5 {
            assert_eq!(bidi.visual_col(i), i);
            assert_eq!(bidi.logical_col(i), i);
        }
    }

    #[test]
    fn rtl_line_basic() {
        // Arabic text: "مرحبا" (hello)
        let text = "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}";
        let line: Vec<Cell> = text.chars().map(cell_narrow).collect();
        let bidi = BidiLine::new(&line);
        assert!(bidi.has_rtl());
        assert!(!bidi.is_identity());
        // The logical order is RTL, so visual should be reversed.
        assert_eq!(bidi.logical_col(0), 4); // first visual position is last logical
        assert_eq!(bidi.logical_col(4), 0); // last visual position is first logical
    }

    #[test]
    fn mixed_ltr_rtl() {
        // "A" + Arabic "مرحبا" + "B"
        let text = format!(
            "A\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}B"
        );
        let line: Vec<Cell> = text.chars().map(cell_narrow).collect();
        let bidi = BidiLine::new(&line);
        assert!(bidi.has_rtl());
        // 'A' and 'B' are LTR, so they should stay at the edges.
        // 'A' should be at visual position 0.
        assert_eq!(bidi.visual_col(0), 0);
        // 'B' should be at the last visual position.
        assert_eq!(bidi.visual_col(6), 6);
    }

    #[test]
    fn wide_char_ltr_line() {
        // A line with a wide CJK character followed by a spacer tail.
        let line: Vec<Cell> = vec![
            cell_wide('汉'),
            cell_spacer_tail(),
            cell_narrow('A'),
            cell_narrow('B'),
        ];
        let bidi = BidiLine::new(&line);
        assert!(!bidi.has_rtl());
        // Identity mapping for LTR
        for i in 0..4 {
            assert_eq!(bidi.visual_col(i), i);
            assert_eq!(bidi.logical_col(i), i);
        }
    }

    #[test]
    fn wide_char_in_rtl_context() {
        // CJK wide char in RTL context
        let text = format!("汉\u{0645}\u{0631}B");
        let mut line: Vec<Cell> = Vec::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '汉' {
                line.push(cell_wide(c));
                line.push(cell_spacer_tail());
            } else {
                line.push(cell_narrow(c));
            }
        }
        let bidi = BidiLine::new(&line);
        // With RTL characters present
        assert!(bidi.has_rtl());
        // The mapping should have exactly as many entries as columns.
        assert_eq!(bidi.visual_to_logical.len(), line.len());
    }

    #[test]
    fn identity_mapping_helper() {
        let mapping = identity_mapping(5);
        assert_eq!(mapping, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn roundtrip_ltr() {
        let line: Vec<Cell> = "ABCDE".chars().map(cell_narrow).collect();
        let bidi = BidiLine::new(&line);
        for logical in 0..5 {
            let visual = bidi.visual_col(logical);
            let back = bidi.logical_col(visual);
            assert_eq!(back, logical, "roundtrip failed for logical col {logical}");
        }
    }

    #[test]
    fn roundtrip_rtl() {
        let text = "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}";
        let line: Vec<Cell> = text.chars().map(cell_narrow).collect();
        let bidi = BidiLine::new(&line);
        assert!(bidi.has_rtl());
        for logical in 0..line.len() {
            let visual = bidi.visual_col(logical);
            let back = bidi.logical_col(visual);
            assert_eq!(
                back, logical,
                "roundtrip failed for logical col {logical}"
            );
        }
    }
}
