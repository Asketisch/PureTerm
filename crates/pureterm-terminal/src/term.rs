use crate::parser::{VteParser, VtePerform, ScreenSnapshot};
use crate::search::{SearchState, SearchMatch, SearchDirection, search_grid};

/// Main terminal structure that combines parser and screen.
pub struct Terminal {
    perform: VtePerform,
    parser: VteParser,
    /// Search state.
    pub search: SearchState,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        Self {
            perform: VtePerform::new(cols, rows, scrollback),
            parser: VteParser::new(),
            search: SearchState::new(),
        }
    }

    /// Write data from the session to the terminal.
    pub fn write(&mut self, data: &[u8]) {
        self.parser.parse(data, &mut self.perform);
    }

    /// Resize the terminal.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.perform.resize(cols, rows);
    }

    /// Check if the screen content has changed since last check.
    pub fn is_dirty(&self) -> bool {
        self.perform.dirty
    }

    /// Clear the dirty flag.
    pub fn clear_dirty(&mut self) {
        self.perform.dirty = false;
    }

    /// Get a snapshot of the current screen state.
    pub fn snapshot(&self) -> ScreenSnapshot {
        self.perform.snapshot()
    }

    /// Get the number of columns.
    pub fn cols(&self) -> usize {
        self.perform.cols
    }

    /// Get the number of rows.
    pub fn rows(&self) -> usize {
        self.perform.rows
    }

    /// Get the terminal title.
    pub fn title(&self) -> &str {
        &self.perform.title
    }

    /// Get the current working directory (from OSC 7).
    pub fn current_dir(&self) -> Option<&str> {
        self.perform.current_dir.as_deref()
    }

    /// Scroll the viewport up.
    pub fn scroll_up(&mut self, lines: usize) {
        self.perform.grid.scroll_viewport_up(lines);
        self.perform.dirty = true;
    }

    /// Scroll the viewport down.
    pub fn scroll_down(&mut self, lines: usize) {
        self.perform.grid.scroll_viewport_down(lines);
        self.perform.dirty = true;
    }

    /// Reset scroll to bottom.
    pub fn scroll_to_bottom(&mut self) {
        self.perform.grid.scroll_to_bottom();
        self.perform.dirty = true;
    }

    /// Get the scroll offset.
    pub fn scroll_offset(&self) -> usize {
        self.perform.grid.scroll_offset()
    }

    /// Get the terminal mode flags (for checking mouse mode, bracketed paste, etc.)
    pub fn mode(&self) -> &crate::mode::TermMode {
        &self.perform.mode
    }

    /// Check if a bell was triggered and clear the flag.
    pub fn take_bell(&mut self) -> bool {
        let triggered = self.perform.bell_triggered;
        if triggered {
            self.perform.bell_triggered = false;
        }
        triggered
    }

    /// Get the history (scrollback) size.
    pub fn history(&self) -> usize {
        self.perform.grid.history()
    }

    /// Generate focus-in sequence (ESC[I). Should be sent when terminal gains focus
    /// and FOCUS_IN_OUT mode (1004) is active.
    pub fn focus_in_sequence() -> &'static [u8] {
        b"\x1b[I"
    }

    /// Generate focus-out sequence (ESC[O). Should be sent when terminal loses focus
    /// and FOCUS_IN_OUT mode (1004) is active.
    pub fn focus_out_sequence() -> &'static [u8] {
        b"\x1b[O"
    }

    /// Check if focus events should be reported.
    pub fn should_report_focus(&self) -> bool {
        self.perform.mode.contains(crate::mode::TermMode::FOCUS_IN_OUT)
    }

    /// Check if modifyOtherKeys mode is active.
    pub fn modify_other_keys(&self) -> bool {
        self.perform.mode.contains(crate::mode::TermMode::MODIFY_OTHER_KEYS)
    }

    /// Get Kitty keyboard protocol flags as a bitmask.
    /// Returns 0 if no Kitty keyboard flags are active.
    pub fn kitty_keyboard_flags(&self) -> u8 {
        let m = &self.perform.mode;
        let mut flags = 0u8;
        if m.contains(crate::mode::TermMode::KITTY_KEYBOARD_DISAMBIGUATE) {
            flags |= 1;
        }
        if m.contains(crate::mode::TermMode::KITTY_KEYBOARD_REPORT_EVENTS) {
            flags |= 2;
        }
        if m.contains(crate::mode::TermMode::KITTY_KEYBOARD_REPORT_ALTERNATES) {
            flags |= 4;
        }
        if m.contains(crate::mode::TermMode::KITTY_KEYBOARD_REPORT_ALL) {
            flags |= 8;
        }
        if m.contains(crate::mode::TermMode::KITTY_KEYBOARD_REPORT_ASSOCIATED) {
            flags |= 16;
        }
        flags
    }

    /// Execute a search with the given query string.
    /// Updates internal search state with all matches.
    pub fn search(&mut self, query: &str) -> Result<(), regex::Error> {
        let re = regex::Regex::new(query)?;
        let visible: Vec<Vec<crate::screen::Cell>> = (0..self.perform.rows)
            .filter_map(|r| self.perform.grid.visible_row(r).map(|r| r.to_vec()))
            .collect();
        // Collect history rows
        let history = self.perform.rows;
        let history_count = self.perform.grid.history();
        let mut history_rows = Vec::with_capacity(history_count);
        for i in 0..history_count {
            if let Some(row) = self.perform.grid.history_row(i) {
                history_rows.push(row.to_vec());
            }
        }
        let all_matches = search_grid(&re, &visible, &history_rows, history);
        self.search.query = Some(re);
        self.search.matches = all_matches;
        self.search.selected = if self.search.matches.is_empty() { None } else { Some(0) };
        Ok(())
    }

    /// Move to the next search match.
    pub fn search_next(&mut self) -> Option<&SearchMatch> {
        if self.search.matches.is_empty() {
            return None;
        }
        let count = self.search.matches.len();
        let current = self.search.selected.unwrap_or(0);
        let next = (current + 1) % count;
        self.search.selected = Some(next);
        self.search.direction = SearchDirection::Forward;
        self.search.matches.get(next)
    }

    /// Move to the previous search match.
    pub fn search_prev(&mut self) -> Option<&SearchMatch> {
        if self.search.matches.is_empty() {
            return None;
        }
        let count = self.search.matches.len();
        let current = self.search.selected.unwrap_or(0);
        let prev = if current == 0 { count - 1 } else { current - 1 };
        self.search.selected = Some(prev);
        self.search.direction = SearchDirection::Backward;
        self.search.matches.get(prev)
    }

    /// Clear the current search.
    pub fn clear_search(&mut self) {
        self.search.clear();
    }

    /// Get the search state (read-only).
    pub fn search_state(&self) -> &SearchState {
        &self.search
    }

    /// Get visible text content for copy.
    pub fn get_text(&self, start_row: usize, start_col: usize, end_row: usize, end_col: usize) -> String {
        // Normalize: ensure start <= end regardless of drag direction
        let (sr, sc, er, ec) = if start_row < end_row || (start_row == end_row && start_col <= end_col) {
            (start_row, start_col, end_row, end_col)
        } else {
            (end_row, end_col, start_row, start_col)
        };
        let mut text = String::new();
        for row in sr..=er.min(self.perform.rows - 1) {
            let col_start = if row == sr { sc } else { 0 };
            let col_end = if row == er { ec.min(self.perform.cols - 1) } else { self.perform.cols - 1 };
            if let Some(row_cells) = self.perform.grid.visible_row(row) {
                for col in col_start..=col_end {
                    if col < row_cells.len() {
                        // Skip spacer cells in text extraction
                        if row_cells[col].wide == crate::screen::WideMode::SpacerTail
                            || row_cells[col].wide == crate::screen::WideMode::SpacerHead
                        {
                            continue;
                        }
                        text.push(row_cells[col].c);
                    }
                }
            }
            if row < er {
                text.push('\n');
            }
        }
        text
    }

    /// Get the full visible text.
    pub fn get_visible_text(&self) -> String {
        let mut text = String::new();
        for row in 0..self.perform.rows {
            if let Some(row_cells) = self.perform.grid.visible_row(row) {
                let mut line = String::new();
                let mut trailing = row_cells.len();
                // Trim trailing spaces (but not spacer tails)
                for (i, cell) in row_cells.iter().enumerate().rev() {
                    if cell.c != ' ' || cell.wide != crate::screen::WideMode::Narrow {
                        trailing = i + 1;
                        break;
                    }
                }
                for cell in &row_cells[..trailing] {
                    // Skip spacer cells
                    if cell.wide == crate::screen::WideMode::SpacerTail
                        || cell.wide == crate::screen::WideMode::SpacerHead
                    {
                        continue;
                    }
                    line.push(cell.c);
                }
                text.push_str(&line);
                if row < self.perform.rows - 1 {
                    text.push('\n');
                }
            }
        }
        text
    }

    /// Check if the terminal is currently on the alternate screen buffer.
    pub fn is_alt_buffer(&self) -> bool {
        self.perform.is_alt_buffer()
    }

    /// Force switch from alternate screen back to primary screen buffer.
    pub fn reset_to_primary_buffer(&mut self) {
        self.perform.switch_to_primary_buffer();
        self.perform.restore_cursor_from_alt();
        self.perform.dirty = true;
    }

    /// Reset all terminal modes to sensible defaults.
    pub fn reset_modes(&mut self) {
        self.perform.mode = crate::mode::TermMode::default();
        self.perform.mode.insert(crate::mode::TermMode::SHOW_CURSOR);
        self.perform.mode.insert(crate::mode::TermMode::AUTO_WRAP);
        self.perform.cursor.visible = true;
        self.perform.dirty = true;
    }
}
