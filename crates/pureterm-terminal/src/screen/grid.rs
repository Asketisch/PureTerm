use crate::mark::Mark;
use crate::screen::cell::Cell;

/// A 2D grid of terminal cells with scrollback history.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Visible rows + scrollback
    rows: Vec<Vec<Cell>>,
    /// Per-row marks (indexed same as `rows`).
    marks: Vec<Option<Mark>>,
    /// Number of visible columns
    cols: usize,
    /// Number of visible rows
    visible_rows: usize,
    /// Maximum scrollback lines
    scrollback_limit: usize,
    /// Current scroll offset (0 = bottom)
    scroll_offset: usize,
}

impl Grid {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let visible_rows = rows;
        let total_rows = rows + scrollback;
        let grid_rows = (0..total_rows)
            .map(|_| (0..cols).map(|_| Cell::blank()).collect())
            .collect();
        let grid_marks = vec![None; total_rows];

        Self {
            rows: grid_rows,
            marks: grid_marks,
            cols,
            visible_rows,
            scrollback_limit: scrollback,
            scroll_offset: 0,
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn visible_rows(&self) -> usize {
        self.visible_rows
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Get the index of the topmost visible row in the internal buffer.
    fn visible_start(&self) -> usize {
        let total = self.rows.len();
        total.saturating_sub(self.visible_rows + self.scroll_offset)
    }

    /// Get a cell at visible coordinates (row, col).
    pub fn get(&self, row: usize, col: usize) -> Option<&Cell> {
        if row >= self.visible_rows || col >= self.cols {
            return None;
        }
        let idx = self.visible_start() + row;
        self.rows.get(idx).and_then(|r| r.get(col))
    }

    /// Get a mutable cell at visible coordinates.
    pub fn get_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        if row >= self.visible_rows || col >= self.cols {
            return None;
        }
        let idx = self.visible_start() + row;
        self.rows.get_mut(idx).and_then(|r| r.get_mut(col))
    }

    /// Scroll up by one line, adding a new blank line at the bottom.
    pub fn scroll_up(&mut self) {
        let new_row = (0..self.cols).map(|_| Cell::blank()).collect();
        // If we're at the scrollback limit, remove the oldest row
        if self.rows.len() > self.visible_rows + self.scrollback_limit {
            self.rows.remove(0);
            self.marks.remove(0);
        }
        self.rows.push(new_row);
        self.marks.push(None);
    }

    /// Scroll down by one line.
    pub fn scroll_down(&mut self) {
        let new_row = (0..self.cols).map(|_| Cell::blank()).collect();
        let total = self.rows.len();
        let start = total.saturating_sub(self.visible_rows);
        if start > 0 {
            self.rows.insert(start, new_row);
            self.marks.insert(start, None);
        }
    }

    /// Resize the grid to new dimensions.
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        // Adjust each row to the new column count
        for row in &mut self.rows {
            row.resize(new_cols, Cell::blank());
        }
        self.cols = new_cols;

        // Adjust visible rows
        if new_rows > self.visible_rows {
            // Add more rows
            let extra = new_rows - self.visible_rows;
            for _ in 0..extra {
                let new_row = (0..self.cols).map(|_| Cell::blank()).collect();
                self.rows.push(new_row);
                self.marks.push(None);
            }
        }
        self.visible_rows = new_rows;

        // Clamp scroll offset
        let max_scroll = self.rows.len().saturating_sub(self.visible_rows);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
    }

    /// Scroll the viewport up (into history).
    pub fn scroll_viewport_up(&mut self, lines: usize) {
        let max_scroll = self.rows.len().saturating_sub(self.visible_rows);
        self.scroll_offset = (self.scroll_offset + lines).min(max_scroll);
    }

    /// Scroll the viewport down.
    pub fn scroll_viewport_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    /// Reset scroll to the bottom.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Get the visible portion of the grid as a flat slice of rows.
    pub fn visible_cells(&self) -> Vec<&[Cell]> {
        let start = self.visible_start();
        let end = start + self.visible_rows;
        self.rows[start..end.min(self.rows.len())]
            .iter()
            .map(|row| row.as_slice())
            .collect()
    }

    /// Get visible row at index.
    pub fn visible_row(&self, row: usize) -> Option<&[Cell]> {
        if row >= self.visible_rows {
            return None;
        }
        let idx = self.visible_start() + row;
        self.rows.get(idx).map(|r| r.as_slice())
    }

    /// Get mutable access to a visible row.
    pub fn visible_row_mut(&mut self, row: usize) -> Option<&mut Vec<Cell>> {
        if row >= self.visible_rows {
            return None;
        }
        let idx = self.visible_start() + row;
        self.rows.get_mut(idx)
    }

    /// Clear the visible area.
    pub fn clear(&mut self) {
        for row_idx in 0..self.visible_rows {
            if let Some(row) = self.visible_row_mut(row_idx) {
                for cell in row.iter_mut() {
                    cell.reset();
                }
            }
        }
    }

    /// Get the scrollback history (rows above visible area).
    pub fn history(&self) -> usize {
        let total = self.rows.len();
        total.saturating_sub(self.visible_rows)
    }

    /// Set a mark on a visible row.
    pub fn set_mark(&mut self, row: usize, mark: Mark) {
        if row >= self.visible_rows {
            return;
        }
        let idx = self.visible_start() + row;
        if let Some(m) = self.marks.get_mut(idx) {
            *m = Some(mark);
        }
    }

    /// Get the mark on a visible row.
    pub fn get_mark(&self, row: usize) -> Option<&Mark> {
        if row >= self.visible_rows {
            return None;
        }
        let idx = self.visible_start() + row;
        self.marks.get(idx).and_then(|m| m.as_ref())
    }

    /// Find the previous row with a mark of the given kind (searching upward).
    /// Returns the visible row index, or None if not found.
    pub fn find_prev_mark(&self, from_row: usize, kind: crate::mark::MarkKind) -> Option<usize> {
        let start = self.visible_start();
        for row in (0..from_row).rev() {
            let idx = start + row;
            if let Some(Some(m)) = self.marks.get(idx) {
                if m.kind == kind {
                    return Some(row);
                }
            }
        }
        None
    }

    /// Find the next row with a mark of the given kind (searching downward).
    pub fn find_next_mark(&self, from_row: usize, kind: crate::mark::MarkKind) -> Option<usize> {
        let start = self.visible_start();
        for row in (from_row + 1)..self.visible_rows {
            let idx = start + row;
            if let Some(Some(m)) = self.marks.get(idx) {
                if m.kind == kind {
                    return Some(row);
                }
            }
        }
        None
    }
}
