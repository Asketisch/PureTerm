//! Grapheme cluster support for cursor movement and selection.
//!
//! Uses the `unicode_segmentation` crate to detect grapheme cluster boundaries
//! within a row of terminal cells. This ensures cursor movement (left/right
//! arrow keys) and text selection treat combined characters (base + combining
//! marks, emoji sequences, etc.) as a single unit.

use crate::screen::cell::WideMode;
use crate::screen::Cell;
use unicode_segmentation::UnicodeSegmentation;

/// A grapheme cluster boundary within a row.
///
/// Represents a single grapheme cluster spanning columns `[start, end)`.
/// For simple characters this is one column; for combined characters it may
/// span multiple cells (base character + combining marks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphemeBoundary {
    /// Start column (inclusive).
    pub start: usize,
    /// End column (exclusive).
    pub end: usize,
}

/// Compute grapheme cluster boundaries for a row of cells.
///
/// Extracts text from non-spacer cells, finds grapheme cluster boundaries
/// using UAX #29, and maps them back to cell column ranges.
///
/// Returns a list of `GraphemeBoundary` in left-to-right order. Every column
/// in the row is covered by exactly one boundary.
pub fn grapheme_boundaries(row: &[Cell]) -> Vec<GraphemeBoundary> {
    let len = row.len();
    if len == 0 {
        return Vec::new();
    }

    // Build the text and track byte-offset-to-column mapping.
    // We skip SpacerTail/SpacerHead cells since they are display-only
    // continuations of a wide character.
    let mut text = String::with_capacity(len);
    let mut byte_to_col: Vec<(usize, usize)> = Vec::new(); // (byte_offset, col)
    let mut spacer_cols: Vec<usize> = Vec::new();

    for (col, cell) in row.iter().enumerate() {
        match cell.wide {
            WideMode::SpacerTail | WideMode::SpacerHead => {
                spacer_cols.push(col);
            }
            WideMode::Narrow | WideMode::Wide => {
                let byte_offset = text.len();
                text.push(cell.c);
                byte_to_col.push((byte_offset, col));
            }
        }
    }

    if text.is_empty() {
        // All cells are spacers; return single boundary covering the whole row.
        return vec![GraphemeBoundary { start: 0, end: len }];
    }

    // Use UnicodeSegmentation to get grapheme cluster byte offsets.
    let grapheme_indices: Vec<(usize, &str)> = text.grapheme_indices(true).collect();

    let mut boundaries = Vec::with_capacity(grapheme_indices.len());

    for (gi, (byte_start, _grapheme)) in grapheme_indices.iter().enumerate() {
        let byte_end = if gi + 1 < grapheme_indices.len() {
            grapheme_indices[gi + 1].0
        } else {
            text.len()
        };

        // Find the column range for this grapheme cluster.
        let col_start = byte_to_col_for(&byte_to_col, *byte_start);
        let col_end = byte_to_col_for(&byte_to_col, byte_end);

        // Include any spacer columns between col_start and the next non-spacer.
        let _col_end_inclusive = if col_end > col_start {
            col_end - 1
        } else {
            col_start
        };

        // Find actual end: include trailing SpacerTail cells.
        let mut actual_end = col_end;
        // If the cell at col_end_inclusive is Wide, check if the next cell
        // is a SpacerTail belonging to it.
        if row[col_start].wide == WideMode::Wide
            && col_start + 1 < len
            && row[col_start + 1].wide == WideMode::SpacerTail
        {
            actual_end = actual_end.max(col_start + 2);
        }

        // Extend to include any adjacent spacer cells at the end.
        while actual_end < len {
            match row[actual_end].wide {
                WideMode::SpacerTail | WideMode::SpacerHead => {
                    actual_end += 1;
                }
                _ => break,
            }
        }

        boundaries.push(GraphemeBoundary {
            start: col_start,
            end: actual_end,
        });
    }

    // Ensure full coverage: any columns not covered by a boundary get merged
    // into the nearest one. In practice this handles stray spacers at edges.
    let mut covered = vec![false; len];
    for b in &boundaries {
        for c in b.start..b.end {
            if c < len {
                covered[c] = true;
            }
        }
    }

    // If there are uncovered spacer columns, extend the boundary before them.
    if let Some(first_uncovered) = covered.iter().position(|&c| !c) {
        // Find the boundary that should absorb these uncovered columns.
        for b in &mut boundaries {
            if b.end > first_uncovered {
                // Extend this boundary backward or forward to cover the gap.
                let gap_end = covered[first_uncovered..]
                    .iter()
                    .position(|&c| c)
                    .map(|p| first_uncovered + p)
                    .unwrap_or(len);
                if b.start > first_uncovered {
                    b.start = first_uncovered;
                }
                b.end = b.end.max(gap_end);
                for c in b.start..b.end.min(len) {
                    covered[c] = true;
                }
            }
        }

        // Final pass: append any remaining uncovered columns.
        let mut i = 0;
        while i < len {
            if !covered[i] {
                let start = i;
                while i < len && !covered[i] {
                    covered[i] = true;
                    i += 1;
                }
                // Merge into adjacent boundary or add new one.
                if let Some(prev) = boundaries.last_mut() {
                    if prev.end == start {
                        prev.end = i;
                        continue;
                    }
                }
                boundaries.push(GraphemeBoundary { start, end: i });
            } else {
                i += 1;
            }
        }
    }

    // Sort boundaries by start column.
    boundaries.sort_by_key(|b| b.start);

    // Merge overlapping boundaries. Adjacent boundaries (where one's end equals
    // the next's start) are NOT overlapping — they are separate grapheme clusters.
    let mut merged: Vec<GraphemeBoundary> = Vec::new();
    for b in boundaries {
        if let Some(last) = merged.last_mut() {
            if b.start < last.end {
                last.end = last.end.max(b.end);
                continue;
            }
        }
        merged.push(b);
    }

    merged
}

/// Map a byte offset to the column index in the original row.
///
/// For a byte offset that matches a character's start byte, returns that
/// character's column. For a byte offset at or past the end of the text,
/// returns one past the last mapped column.
fn byte_to_col_for(mapping: &[(usize, usize)], byte_offset: usize) -> usize {
    if mapping.is_empty() {
        return 0;
    }
    // Find the first entry with byte_offset > target.
    let idx = mapping.partition_point(|(boff, _)| *boff < byte_offset);
    if idx < mapping.len() {
        // mapping[idx].0 is the first byte_offset >= byte_offset.
        // If it matches exactly, this character starts at byte_offset.
        // If it's greater, then byte_offset falls within the previous character,
        // and we return that character's column.
        mapping[idx].1
    } else {
        // byte_offset is past all character start positions — it's at the end.
        // Return one past the last column.
        mapping.last().map(|&(_, col)| col + 1).unwrap_or(0)
    }
}

/// Find the start column of the grapheme cluster containing column `col`.
///
/// If `col` is in the middle of a grapheme cluster (e.g., on a spacer cell or
/// a combining mark), this returns the start of that cluster. If `col` is at
/// the start of a cluster, it returns `col`.
///
/// Returns `col` unchanged if the row is empty or `col` is out of bounds.
pub fn prev_grapheme_boundary(row: &[Cell], col: usize) -> usize {
    if row.is_empty() || col >= row.len() {
        return col.min(row.len().saturating_sub(1));
    }

    let boundaries = grapheme_boundaries(row);

    // Find the boundary containing `col`.
    for b in &boundaries {
        if b.start <= col && col < b.end {
            return b.start;
        }
    }

    // If col is at the very end (== row.len()), find the last boundary.
    if col == row.len() {
        return boundaries.last().map(|b| b.start).unwrap_or(col);
    }

    col
}

/// Find the start column of the next grapheme cluster after column `col`.
///
/// Returns the start of the next cluster past the one containing `col`.
/// If `col` is in the last cluster, returns `row.len()`.
pub fn next_grapheme_boundary(row: &[Cell], col: usize) -> usize {
    if row.is_empty() {
        return 0;
    }

    let boundaries = grapheme_boundaries(row);

    // Find the boundary containing `col`.
    for (i, b) in boundaries.iter().enumerate() {
        if b.start <= col && col < b.end {
            if i + 1 < boundaries.len() {
                return boundaries[i + 1].start;
            } else {
                // Last cluster; return its end.
                return b.end;
            }
        }
        // If col is before this boundary, this boundary is the next one.
        if col < b.start {
            return b.start;
        }
    }

    // col is past all boundaries (e.g., at row.len()).
    row.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::cell::WideMode;

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
    fn empty_row() {
        let row: Vec<Cell> = vec![];
        let bounds = grapheme_boundaries(&row);
        assert!(bounds.is_empty());
    }

    #[test]
    fn simple_ascii() {
        let row: Vec<Cell> = "ABC".chars().map(cell_narrow).collect();
        let bounds = grapheme_boundaries(&row);
        assert_eq!(bounds.len(), 3);
        assert_eq!(bounds[0], GraphemeBoundary { start: 0, end: 1 });
        assert_eq!(bounds[1], GraphemeBoundary { start: 1, end: 2 });
        assert_eq!(bounds[2], GraphemeBoundary { start: 2, end: 3 });
    }

    #[test]
    fn combining_accent() {
        // 'e' + COMBINING ACUTE ACCENT is one grapheme cluster.
        let row: Vec<Cell> = vec![cell_narrow('e'), cell_narrow('\u{0301}')];
        let bounds = grapheme_boundaries(&row);
        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0], GraphemeBoundary { start: 0, end: 2 });
    }

    #[test]
    fn wide_char_with_spacer() {
        let row: Vec<Cell> = vec![cell_wide('漢'), cell_spacer_tail()];
        let bounds = grapheme_boundaries(&row);
        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0], GraphemeBoundary { start: 0, end: 2 });
    }

    #[test]
    fn wide_char_mixed() {
        // "漢A" -> Wide + SpacerTail + Narrow
        let row: Vec<Cell> = vec![cell_wide('漢'), cell_spacer_tail(), cell_narrow('A')];
        let bounds = grapheme_boundaries(&row);
        assert_eq!(bounds.len(), 2);
        assert_eq!(bounds[0], GraphemeBoundary { start: 0, end: 2 });
        assert_eq!(bounds[1], GraphemeBoundary { start: 2, end: 3 });
    }

    #[test]
    fn combining_with_wide() {
        // Wide char + combining mark + regular char
        let row: Vec<Cell> = vec![
            cell_wide('漢'),
            cell_spacer_tail(),
            cell_narrow('\u{0301}'), // combining acute
            cell_narrow('B'),
        ];
        let bounds = grapheme_boundaries(&row);
        // The wide char + combining mark form one grapheme, 'B' is another.
        assert_eq!(bounds.len(), 2);
        assert_eq!(bounds[0].start, 0);
        assert!(bounds[0].end >= 2);
        assert_eq!(bounds[1].start, 3);
        assert_eq!(bounds[1].end, 4);
    }

    #[test]
    fn prev_boundary_simple() {
        let row: Vec<Cell> = "ABC".chars().map(cell_narrow).collect();
        assert_eq!(prev_grapheme_boundary(&row, 0), 0);
        assert_eq!(prev_grapheme_boundary(&row, 1), 1);
        assert_eq!(prev_grapheme_boundary(&row, 2), 2);
    }

    #[test]
    fn prev_boundary_combining() {
        let row: Vec<Cell> = vec![cell_narrow('e'), cell_narrow('\u{0301}'), cell_narrow('X')];
        // Both columns 0 and 1 belong to the same grapheme cluster starting at 0.
        assert_eq!(prev_grapheme_boundary(&row, 0), 0);
        assert_eq!(prev_grapheme_boundary(&row, 1), 0);
        assert_eq!(prev_grapheme_boundary(&row, 2), 2);
    }

    #[test]
    fn next_boundary_simple() {
        let row: Vec<Cell> = "ABC".chars().map(cell_narrow).collect();
        assert_eq!(next_grapheme_boundary(&row, 0), 1);
        assert_eq!(next_grapheme_boundary(&row, 1), 2);
        assert_eq!(next_grapheme_boundary(&row, 2), 3); // past end
    }

    #[test]
    fn next_boundary_combining() {
        let row: Vec<Cell> = vec![cell_narrow('e'), cell_narrow('\u{0301}'), cell_narrow('X')];
        assert_eq!(next_grapheme_boundary(&row, 0), 2);
        assert_eq!(next_grapheme_boundary(&row, 1), 2);
        assert_eq!(next_grapheme_boundary(&row, 2), 3);
    }

    #[test]
    fn prev_next_roundtrip() {
        let row: Vec<Cell> = "Hello".chars().map(cell_narrow).collect();
        for col in 0..row.len() {
            let prev = prev_grapheme_boundary(&row, col);
            let next = next_grapheme_boundary(&row, prev);
            if col < row.len() - 1 {
                assert!(next > prev, "next should advance past prev at col {col}");
            }
        }
    }

    #[test]
    fn emoji_sequence() {
        // Family emoji: WOMAN + ZWJ + HEART + ZWJ + WOMAN
        // Each is a separate cell in the terminal.
        let row: Vec<Cell> = vec![
            cell_narrow('\u{1F469}'), // WOMAN
            cell_narrow('\u{200D}'),  // ZWJ
            cell_narrow('\u{2764}'),  // HEART
            cell_narrow('\u{200D}'),  // ZWJ
            cell_narrow('\u{1F469}'), // WOMAN
        ];
        let bounds = grapheme_boundaries(&row);
        // The entire ZWJ sequence should be one grapheme cluster.
        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0], GraphemeBoundary { start: 0, end: 5 });
    }

    #[test]
    fn multiple_clusters_with_combining() {
        // "e\u{0301}a\u{0308}bc" -> 4 grapheme clusters
        let row: Vec<Cell> = vec![
            cell_narrow('e'),
            cell_narrow('\u{0301}'), // combining acute
            cell_narrow('a'),
            cell_narrow('\u{0308}'), // combining diaeresis
            cell_narrow('b'),
            cell_narrow('c'),
        ];
        let bounds = grapheme_boundaries(&row);
        assert_eq!(bounds.len(), 4);
        assert_eq!(bounds[0], GraphemeBoundary { start: 0, end: 2 }); // e + acute
        assert_eq!(bounds[1], GraphemeBoundary { start: 2, end: 4 }); // a + diaeresis
        assert_eq!(bounds[2], GraphemeBoundary { start: 4, end: 5 }); // b
        assert_eq!(bounds[3], GraphemeBoundary { start: 5, end: 6 }); // c
    }

    #[test]
    fn single_cell_row() {
        let row: Vec<Cell> = vec![cell_narrow('X')];
        let bounds = grapheme_boundaries(&row);
        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0], GraphemeBoundary { start: 0, end: 1 });

        assert_eq!(prev_grapheme_boundary(&row, 0), 0);
        assert_eq!(next_grapheme_boundary(&row, 0), 1);
    }

    #[test]
    fn out_of_bounds_col() {
        let row: Vec<Cell> = "AB".chars().map(cell_narrow).collect();
        // col == row.len() should be handled gracefully.
        assert_eq!(prev_grapheme_boundary(&row, 2), 1);
        assert_eq!(next_grapheme_boundary(&row, 2), 2);
    }
}
