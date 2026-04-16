//! Terminal search system.
//!
//! Supports regex search across visible content and scrollback.
//! Based on Ghostty's terminal/search/ architecture.

use regex::Regex;

/// Position in the grid (row, col).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridPos {
    pub row: usize,
    pub col: usize,
}

impl GridPos {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

/// A single search match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Start position of the match.
    pub start: GridPos,
    /// End position of the match (inclusive).
    pub end: GridPos,
}

impl SearchMatch {
    pub fn new(start: GridPos, end: GridPos) -> Self {
        Self { start, end }
    }
}

/// Search direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

/// Search state tracking.
#[derive(Debug, Clone)]
pub struct SearchState {
    /// Compiled regex query.
    pub(crate) query: Option<Regex>,
    /// All matches found.
    pub(crate) matches: Vec<SearchMatch>,
    /// Index of currently selected match.
    pub(crate) selected: Option<usize>,
    /// Last search direction.
    pub(crate) direction: SearchDirection,
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: None,
            matches: Vec::new(),
            selected: None,
            direction: SearchDirection::Forward,
        }
    }

    /// Get the current query string.
    pub fn query_str(&self) -> Option<&str> {
        self.query.as_ref().map(|r| r.as_str())
    }

    /// Get all matches.
    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    /// Get the selected match index.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Get the selected match.
    pub fn selected_match(&self) -> Option<&SearchMatch> {
        self.selected.and_then(|i| self.matches.get(i))
    }

    /// Check if search is active.
    pub fn is_active(&self) -> bool {
        self.query.is_some()
    }

    /// Number of matches.
    pub fn count(&self) -> usize {
        self.matches.len()
    }

    /// Clear the search state.
    pub fn clear(&mut self) {
        self.query = None;
        self.matches.clear();
        self.selected = None;
    }
}

/// Extract text from a grid row, skipping spacer cells.
fn extract_row_text(cells: &[crate::screen::Cell]) -> String {
    let mut text = String::with_capacity(cells.len());
    for cell in cells {
        if cell.wide == crate::screen::WideMode::SpacerTail
            || cell.wide == crate::screen::WideMode::SpacerHead
        {
            continue;
        }
        text.push(cell.c);
    }
    text
}

/// Get trailing length (excluding trailing spaces).
fn trim_trailing_len(text: &str) -> usize {
    text.trim_end().len()
}

/// Build a char-position map for a row: maps char index to column index.
fn build_char_to_col_map(cells: &[crate::screen::Cell]) -> Vec<usize> {
    let mut map = Vec::with_capacity(cells.len());
    for (col, cell) in cells.iter().enumerate() {
        if cell.wide != crate::screen::WideMode::SpacerTail
            && cell.wide != crate::screen::WideMode::SpacerHead
        {
            map.push(col);
        }
    }
    map
}

/// Perform a regex search on visible rows and scrollback.
///
/// Returns a list of matches with grid positions.
pub fn search_grid(
    regex: &Regex,
    visible_cells: &[Vec<crate::screen::Cell>],
    history_rows: &[Vec<crate::screen::Cell>],
    visible_rows: usize,
) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    let history_len = history_rows.len();

    // Search history rows (oldest to newest)
    for (i, row) in history_rows.iter().enumerate() {
        let row_idx = history_len - 1 - i; // history rows are indexed from top
        // History rows use negative offset from visible area
        // We store them as (visible_rows + offset)
        let text = extract_row_text(row);
        let col_map = build_char_to_col_map(row);
        find_matches_in_row(regex, &text, &col_map, row_idx + visible_rows, &mut matches);
    }

    // Search visible rows
    for (row_idx, row) in visible_cells.iter().enumerate() {
        let text = extract_row_text(row);
        let col_map = build_char_to_col_map(row);
        find_matches_in_row(regex, &text, &col_map, row_idx, &mut matches);
    }

    matches
}

/// Search only visible rows.
pub fn search_visible(
    regex: &Regex,
    visible_cells: &[Vec<crate::screen::Cell>],
) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    for (row_idx, row) in visible_cells.iter().enumerate() {
        let text = extract_row_text(row);
        let col_map = build_char_to_col_map(row);
        find_matches_in_row(regex, &text, &col_map, row_idx, &mut matches);
    }
    matches
}

/// Find all matches in a single row of text.
fn find_matches_in_row(
    regex: &Regex,
    text: &str,
    col_map: &[usize],
    row: usize,
    results: &mut Vec<SearchMatch>,
) {
    let trimmed_len = trim_trailing_len(text);
    if trimmed_len == 0 {
        return;
    }
    let search_text = &text[..trimmed_len];

    for mat in regex.find_iter(search_text) {
        let start_char = mat.start();
        let end_char = mat.end().saturating_sub(1); // inclusive end

        let start_col = col_map.get(start_char).copied().unwrap_or(start_char);
        let end_col = col_map.get(end_char).copied().unwrap_or(end_char);

        results.push(SearchMatch::new(
            GridPos::new(row, start_col),
            GridPos::new(row, end_col),
        ));
    }
}
