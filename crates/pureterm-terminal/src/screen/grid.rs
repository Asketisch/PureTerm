use crate::mark::Mark;
use crate::screen::cell::Cell;

/// Per-row double-width/double-height mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DoubleMode {
    #[default]
    /// Normal single-width, single-height line.
    Normal,
    /// Double-width line (DECDWL, CSI # 6).
    DoubleWidth,
    /// Double-height line, top half (DECDHL, CSI # 3).
    DoubleHeightTop,
    /// Double-height line, bottom half (DECDHL, CSI # 4).
    DoubleHeightBottom,
}

/// A 2D grid of terminal cells with scrollback history.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Visible rows + scrollback
    rows: Vec<Vec<Cell>>,
    /// Per-row marks (indexed same as `rows`).
    marks: Vec<Option<Mark>>,
    /// Per-row wrapped flag (true if line was soft-wrapped from the previous line).
    wrapped: Vec<bool>,
    /// Per-row double-width/double-height mode.
    double_mode: Vec<DoubleMode>,
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
        let grid_rows = (0..visible_rows)
            .map(|_| (0..cols).map(|_| Cell::blank()).collect())
            .collect();
        let grid_marks = vec![None; visible_rows];
        let grid_wrapped = vec![false; visible_rows];
        let grid_double = vec![DoubleMode::Normal; visible_rows];

        Self {
            rows: grid_rows,
            marks: grid_marks,
            wrapped: grid_wrapped,
            double_mode: grid_double,
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
        if self.rows.len() > self.visible_rows + self.scrollback_limit {
            self.rows.remove(0);
            self.marks.remove(0);
            self.wrapped.remove(0);
            self.double_mode.remove(0);
        }
        self.rows.push(new_row);
        self.marks.push(None);
        self.wrapped.push(false);
        self.double_mode.push(DoubleMode::Normal);
    }

    /// Scroll down by one line.
    pub fn scroll_down(&mut self) {
        let new_row = (0..self.cols).map(|_| Cell::blank()).collect();
        let total = self.rows.len();
        let start = total.saturating_sub(self.visible_rows);
        if start > 0 {
            self.rows.insert(start, new_row);
            self.marks.insert(start, None);
            self.wrapped.insert(start, false);
            self.double_mode.insert(start, DoubleMode::Normal);
        }
        if self.rows.len() > self.visible_rows + self.scrollback_limit {
            self.rows.remove(0);
            self.marks.remove(0);
            self.wrapped.remove(0);
            self.double_mode.remove(0);
        }
    }

    /// Scroll a specific region up by one line (delete top row, add blank at bottom of region).
    /// `top` and `bottom` are 0-based visible row indices, exclusive of bottom.
    pub fn scroll_region_up(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom > self.visible_rows {
            return;
        }
        let start = self.visible_start();
        // Remove the row at the top of the region
        let remove_idx = start + top;
        if remove_idx < self.rows.len() {
            self.rows.remove(remove_idx);
            self.marks.remove(remove_idx);
            self.wrapped.remove(remove_idx);
        }
        // Insert a blank row just before the bottom of the region
        let insert_idx = start + bottom - 1; // -1 because we removed a row
        let new_row = (0..self.cols).map(|_| Cell::blank()).collect();
        if insert_idx <= self.rows.len() {
            self.rows.insert(insert_idx, new_row);
            self.marks.insert(insert_idx, None);
            self.wrapped.insert(insert_idx, false);
        }
    }

    /// Scroll a specific region down by one line (insert blank at top, delete bottom row).
    /// `top` and `bottom` are 0-based visible row indices, exclusive of bottom.
    pub fn scroll_region_down(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom > self.visible_rows {
            return;
        }
        let start = self.visible_start();
        // Insert a blank row at the top of the region
        let insert_idx = start + top;
        let new_row = (0..self.cols).map(|_| Cell::blank()).collect();
        if insert_idx <= self.rows.len() {
            self.rows.insert(insert_idx, new_row);
            self.marks.insert(insert_idx, None);
            self.wrapped.insert(insert_idx, false);
        }
        // Remove the row at the bottom of the region
        let remove_idx = start + bottom; // bottom row (1 past because we inserted)
        if remove_idx < self.rows.len() {
            self.rows.remove(remove_idx);
            self.marks.remove(remove_idx);
            self.wrapped.remove(remove_idx);
        }
    }

    /// Resize the grid to new dimensions with reflow support.
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        if new_cols == self.cols && new_rows == self.visible_rows {
            return;
        }

        // Reflow: recombine wrapped lines and re-wrap at the new width
        if new_cols != self.cols {
            self.reflow(new_cols);
        } else {
            // Just resize each row
            for row in &mut self.rows {
                row.resize(new_cols, Cell::blank());
            }
        }
        self.cols = new_cols;

        // Adjust visible rows
        if new_rows > self.visible_rows {
            let extra = new_rows - self.visible_rows;
            for _ in 0..extra {
                self.rows.push((0..self.cols).map(|_| Cell::blank()).collect());
                self.marks.push(None);
                self.wrapped.push(false);
            }
        }
        self.visible_rows = new_rows;

        // Clamp scroll offset
        let max_scroll = self.rows.len().saturating_sub(self.visible_rows);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
    }

    /// Reflow rows: recombine wrapped lines, then re-wrap at new width.
    fn reflow(&mut self, new_cols: usize) {
        // Step 1: Recombine wrapped lines into logical lines
        let mut logical_lines: Vec<Vec<Cell>> = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            let mut line = std::mem::take(&mut self.rows[i]);
            i += 1;
            // If the next row is wrapped, append it
            while i < self.rows.len() && self.wrapped[i] {
                let wrapped_row = std::mem::take(&mut self.rows[i]);
                line.extend(wrapped_row);
                i += 1;
            }
            logical_lines.push(line);
        }

        // Step 2: Re-wrap each logical line at the new width
        let mut new_rows: Vec<Vec<Cell>> = Vec::new();
        let mut new_wrapped: Vec<bool> = Vec::new();
        let mut new_marks: Vec<Option<Mark>> = Vec::new();

        for (line_idx, line) in logical_lines.into_iter().enumerate() {
            if line.is_empty() || line.iter().all(|c| c.c == ' ') {
                new_rows.push((0..new_cols).map(|_| Cell::blank()).collect());
                new_wrapped.push(false);
                // Preserve mark for the first row of the logical line
                new_marks.push(self.marks.get(line_idx).and_then(|m| m.clone()));
            } else {
                let chunks = line.chunks(new_cols);
                let num_chunks = chunks.len();
                for (chunk_idx, chunk) in chunks.enumerate() {
                    let mut row: Vec<Cell> = chunk.to_vec();
                    row.resize(new_cols, Cell::blank());
                    new_rows.push(row);
                    new_wrapped.push(chunk_idx < num_chunks - 1);
                    new_marks.push(None); // marks only on first row
                }
                // Put the mark on the first chunk
                if let Some(last) = new_marks.last_mut() {
                    *last = self.marks.get(line_idx).and_then(|m| m.clone());
                }
            }
        }

        self.rows = new_rows;
        self.wrapped = new_wrapped;
        self.marks = new_marks;
    }

    /// Set the wrapped flag for a visible row.
    pub fn set_wrapped(&mut self, row: usize, value: bool) {
        if row >= self.visible_rows {
            return;
        }
        let idx = self.visible_start() + row;
        if let Some(w) = self.wrapped.get_mut(idx) {
            *w = value;
        }
    }

    /// Get the wrapped flag for a visible row.
    pub fn is_wrapped(&self, row: usize) -> bool {
        if row >= self.visible_rows {
            return false;
        }
        let idx = self.visible_start() + row;
        self.wrapped.get(idx).copied().unwrap_or(false)
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

    /// Get a history row by index (0 = oldest history row).
    pub fn history_row(&self, idx: usize) -> Option<&[Cell]> {
        let history_count = self.history();
        if idx >= history_count {
            return None;
        }
        // History rows are above the visible area: rows[0..history_count]
        // idx=0 is the oldest (rows[0]), idx=history_count-1 is newest
        self.rows.get(idx).map(|r| r.as_slice())
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

    /// Clear scrollback history (CSI 3 J).
    pub fn clear_history(&mut self) {
        // Remove all rows above the visible area
        let visible_start = self.rows.len().saturating_sub(self.visible_rows);
        if visible_start > 0 {
            self.rows.drain(0..visible_start);
            self.marks.drain(0..visible_start);
            self.wrapped.drain(0..visible_start);
            self.double_mode.drain(0..visible_start);
        }
        self.scroll_offset = 0;
    }

    /// Set the double-width/double-height mode for a visible row.
    pub fn set_double_mode(&mut self, row: usize, mode: DoubleMode) {
        if row >= self.visible_rows {
            return;
        }
        let idx = self.visible_start() + row;
        if let Some(m) = self.double_mode.get_mut(idx) {
            *m = mode;
        }
    }

    /// Get the double-width/double-height mode for a visible row.
    pub fn get_double_mode(&self, row: usize) -> DoubleMode {
        if row >= self.visible_rows {
            return DoubleMode::Normal;
        }
        let idx = self.visible_start() + row;
        self.double_mode.get(idx).copied().unwrap_or_default()
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
