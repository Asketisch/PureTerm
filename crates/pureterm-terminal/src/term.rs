use crate::parser::{VteParser, VtePerform, ScreenSnapshot};

/// Main terminal structure that combines parser and screen.
pub struct Terminal {
    perform: VtePerform,
    parser: VteParser,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        Self {
            perform: VtePerform::new(cols, rows, scrollback),
            parser: VteParser::new(),
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

    /// Get the history (scrollback) size.
    pub fn history(&self) -> usize {
        self.perform.grid.history()
    }

    /// Get visible text content for copy.
    pub fn get_text(&self, start_row: usize, start_col: usize, end_row: usize, end_col: usize) -> String {
        let mut text = String::new();
        for row in start_row..=end_row.min(self.perform.rows - 1) {
            let col_start = if row == start_row { start_col } else { 0 };
            let col_end = if row == end_row { end_col.min(self.perform.cols - 1) } else { self.perform.cols - 1 };
            if let Some(row_cells) = self.perform.grid.visible_row(row) {
                for col in col_start..=col_end {
                    if col < row_cells.len() {
                        text.push(row_cells[col].c);
                    }
                }
            }
            if row < end_row {
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
                // Trim trailing spaces
                for (i, cell) in row_cells.iter().enumerate().rev() {
                    if cell.c != ' ' {
                        trailing = i + 1;
                        break;
                    }
                }
                for cell in &row_cells[..trailing] {
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
}
