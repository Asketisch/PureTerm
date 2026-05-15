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

    /// Set the response writer callback for terminal query replies (DA/DSR/DECRQSS).
    /// The callback receives bytes that should be written to the child process's stdin.
    pub fn set_response_writer(&mut self, writer: Box<dyn FnMut(&[u8]) + Send>) {
        self.perform.set_response_writer(writer);
    }

    /// Set the clipboard callback for OSC 52 clipboard operations.
    /// The callback receives decoded text that should be set on the system clipboard.
    pub fn set_clipboard_callback(&mut self, callback: Box<dyn FnMut(&str) + Send>) {
        self.perform.set_clipboard_callback(callback);
    }

    /// Set the notification callback for OSC 9 / OSC 777 desktop notifications.
    /// The callback receives (title, body).
    pub fn set_notification_callback(&mut self, callback: Box<dyn FnMut(&str, &str) + Send>) {
        self.perform.set_notification_callback(callback);
    }

    /// Set the mark callback for OSC 133 shell integration marks.
    /// The callback receives the mark kind (PromptStart, CommandStart, OutputStart, OutputEnd).
    pub fn set_mark_callback(&mut self, callback: Box<dyn FnMut(crate::mark::MarkKind) + Send>) {
        self.perform.set_mark_callback(callback);
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

    /// Check if the cursor is visible (visible flag + SHOW_CURSOR mode).
    pub fn cursor_visible(&self) -> bool {
        self.perform.cursor.visible && self.perform.mode.contains(crate::mode::TermMode::SHOW_CURSOR)
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

    /// Scroll to an absolute position.
    pub fn scroll_to_position(&mut self, offset: usize) {
        self.perform.grid.scroll_to_position(offset);
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

    /// Re-run the active query if tail-find is enabled. Cheap no-op otherwise.
    /// Returns the current number of matches.
    pub fn tail_find_refresh(&mut self) -> usize {
        if !self.search.tail_find {
            return self.search.matches.len();
        }
        let query = match self.search.query.as_ref() {
            Some(re) => re.as_str().to_string(),
            None => return 0,
        };
        let _ = self.search(&query);
        self.search.matches.len()
    }

    /// Get visible text content for copy (linear selection).
    pub fn get_text(&self, start_row: usize, start_col: usize, end_row: usize, end_col: usize) -> String {
        self.get_text_impl(start_row, start_col, end_row, end_col, false)
    }

    /// Get visible text content for copy (block/rectangular selection).
    /// For block selection, each row extracts only the column range independently.
    pub fn get_text_block(&self, start_row: usize, start_col: usize, end_row: usize, end_col: usize) -> String {
        self.get_text_impl(start_row, start_col, end_row, end_col, true)
    }

    /// Internal text extraction supporting both linear and block modes.
    fn get_text_impl(&self, start_row: usize, start_col: usize, end_row: usize, end_col: usize, block: bool) -> String {
        // Normalize: ensure start <= end regardless of drag direction
        let (sr, sc, er, ec) = if start_row < end_row || (start_row == end_row && start_col <= end_col) {
            (start_row, start_col, end_row, end_col)
        } else {
            (end_row, end_col, start_row, start_col)
        };

        // For block selection, normalize column range independently
        let (col_min, col_max) = if block {
            (sc.min(ec), sc.max(ec))
        } else {
            (sc, ec)
        };

        let mut text = String::new();
        for row in sr..=er.min(self.perform.rows - 1) {
            let (col_start, col_end) = if block {
                // Block: same column range on every row
                (col_min, col_max.min(self.perform.cols - 1))
            } else {
                // Linear: full-line span between start and end
                let cs = if row == sr { sc } else { 0 };
                let ce = if row == er { ec.min(self.perform.cols - 1) } else { self.perform.cols - 1 };
                (cs, ce)
            };

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

    /// Get a reference to the block manager.
    pub fn block_manager(&self) -> &crate::block::BlockManager {
        &self.perform.block_manager
    }

    /// Get a mutable reference to the block manager.
    pub fn block_manager_mut(&mut self) -> &mut crate::block::BlockManager {
        &mut self.perform.block_manager
    }

    /// Navigate to the next command block prompt row.
    pub fn next_block_prompt(&self, current_row: usize) -> Option<usize> {
        self.perform.block_manager.next_prompt_row(current_row)
    }

    /// Navigate to the previous command block prompt row.
    pub fn prev_block_prompt(&self, current_row: usize) -> Option<usize> {
        self.perform.block_manager.prev_prompt_row(current_row)
    }

    /// Get total grid row count (visible + scrollback).
    pub fn grid_rows(&self) -> usize {
        self.perform.grid.total_rows()
    }

    /// Get grid column count.
    pub fn grid_cols(&self) -> usize {
        self.perform.cols
    }

    /// Get total image storage bytes.
    pub fn image_bytes(&self) -> usize {
        self.perform.image_manager.used_bytes()
    }

    /// Drain the queue of completed lines (LF + wrap).
    /// Pane calls this each frame after `write()` to feed triggers.
    pub fn drain_completed_lines(&mut self) -> Vec<crate::parser::perform::CompletedLine> {
        std::mem::take(&mut self.perform.completed_lines).into_iter().collect()
    }

    /// Set background color across a range of cells in the row identified by `line_id`.
    /// Silently no-ops if the line has been evicted from scrollback.
    pub fn apply_highlight(&mut self, line_id: u64, range: std::ops::Range<usize>, bg: crate::screen::Color) {
        let cols = self.cols();
        let Some(row) = self.perform.grid.row_mut_by_line_id(line_id) else { return; };
        let end = range.end.min(cols).min(row.len());
        for col in range.start..end {
            row[col].bg = bg;
        }
        self.perform.dirty = true;
    }

    /// Attach a hyperlink across a range in the row identified by `line_id`.
    pub fn apply_hyperlink(&mut self, line_id: u64, range: std::ops::Range<usize>, url: &str) {
        use std::sync::Arc;
        let link = Arc::new(crate::hyperlink::Hyperlink::new(url.to_string(), None));
        let cols = self.cols();
        let Some(row) = self.perform.grid.row_mut_by_line_id(line_id) else { return; };
        let end = range.end.min(cols).min(row.len());
        for col in range.start..end {
            row[col].hyperlink = Some(link.clone());
        }
        self.perform.dirty = true;
    }

    /// Mark a range as protected (used by PasswordMask trigger; downstream rendering
    /// substitutes the glyph via secret_redaction visual policy).
    pub fn apply_password_mask(&mut self, line_id: u64, range: std::ops::Range<usize>) {
        let cols = self.cols();
        let Some(row) = self.perform.grid.row_mut_by_line_id(line_id) else { return; };
        let end = range.end.min(cols).min(row.len());
        for col in range.start..end {
            row[col].protected = true;
        }
        self.perform.dirty = true;
    }
}

#[cfg(test)]
mod term_trigger_tests {
    use super::*;
    use crate::screen::Color;

    #[test]
    fn drain_returns_completed_lines() {
        let mut t = Terminal::new(10, 3, 100);
        t.write(b"hello\r\nworld\r\n");
        let lines = t.drain_completed_lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text.trim_end(), "hello");
        assert_eq!(lines[1].text.trim_end(), "world");
        assert!(t.drain_completed_lines().is_empty());
    }

    #[test]
    fn apply_highlight_writes_bg() {
        let mut t = Terminal::new(20, 3, 100);
        t.write(b"error here\n");
        let line = t.drain_completed_lines().pop().expect("LF should enqueue");
        let red = Color::Palette(9);
        t.apply_highlight(line.line_id, 0..5, red);
        let ss = t.snapshot();
        let row_with_text = ss.rows.iter().find(|r| r.iter().any(|c| c.c == 'e')).expect("text row");
        assert_eq!(row_with_text[0].bg, red);
    }

    #[test]
    fn apply_hyperlink_attaches_url() {
        let mut t = Terminal::new(20, 3, 100);
        t.write(b"link me\n");
        let line = t.drain_completed_lines().pop().unwrap();
        t.apply_hyperlink(line.line_id, 0..7, "https://example.com");
        let ss = t.snapshot();
        let row = ss.rows.iter().find(|r| r.iter().any(|c| c.c == 'l')).unwrap();
        assert!(row[0].hyperlink.is_some());
        assert_eq!(row[0].hyperlink.as_ref().unwrap().uri, "https://example.com");
    }

    #[test]
    fn apply_password_mask_marks_protected() {
        let mut t = Terminal::new(20, 3, 100);
        t.write(b"secret123\n");
        let line = t.drain_completed_lines().pop().unwrap();
        t.apply_password_mask(line.line_id, 0..9);
        let ss = t.snapshot();
        let row = ss.rows.iter().find(|r| r.iter().any(|c| c.c == 's')).unwrap();
        assert!(row[0].protected);
    }

    #[test]
    fn apply_highlight_to_unknown_id_is_noop() {
        let mut t = Terminal::new(10, 2, 5);
        // Should not panic.
        t.apply_highlight(u64::MAX, 0..5, Color::Palette(1));
    }

    #[test]
    fn tail_find_refresh_picks_up_new_matches() {
        let mut t = Terminal::new(5, 3, 100);
        t.search("foo").expect("compile");
        assert_eq!(t.search_state().count(), 0);
        t.search.enable_tail_find(true);
        t.write(b"foo\n");
        let n = t.tail_find_refresh();
        assert_eq!(n, 1);
        assert_eq!(t.search_state().count(), 1);
    }
}
