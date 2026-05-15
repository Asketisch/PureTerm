//! Vim-style modal copy mode for keyboard-driven scrollback navigation
//! and selection.
//!
//! `CopyMode` is a pure state machine: it owns the cursor, selection, and
//! viewport dimensions, but does not handle key events or interact with the
//! pane/grid directly. The owning pane translates key events into method
//! calls and is responsible for clamping `row` to its actual scrollback
//! depth (this layer only enforces the visible-viewport upper bound).
//!
//! Modeled on iTerm2's `sources/CopyMode/` and tmux's copy-mode.

/// Cursor position relative to the visible top-left of the viewport.
/// Negative `row` indicates a scroll-back position (rows above the visible
/// area); positive values index into the visible viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CopyCursor {
    pub row: i64,
    pub col: usize,
}

/// Active selection anchored at `anchor` and extending to `head`. The two
/// endpoints are normalised on read via [`CopyMode::normalised_selection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopySelection {
    None,
    Linear { anchor: CopyCursor, head: CopyCursor },
    Block { anchor: CopyCursor, head: CopyCursor },
}

impl Default for CopySelection {
    fn default() -> Self {
        CopySelection::None
    }
}

/// Word-movement step (column delta). Real word-boundary detection lives
/// in the pane layer where graphemes are available; this is a deliberate
/// simplification.
const WORD_STEP: usize = 5;

#[derive(Default, Debug)]
pub struct CopyMode {
    pub active: bool,
    pub cursor: CopyCursor,
    pub selection: CopySelection,
    pub viewport_rows: usize,
    pub viewport_cols: usize,
}

impl CopyMode {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enter(&mut self, viewport_rows: usize, viewport_cols: usize) {
        self.active = true;
        self.cursor = CopyCursor { row: 0, col: 0 };
        self.selection = CopySelection::None;
        self.viewport_rows = viewport_rows;
        self.viewport_cols = viewport_cols;
    }

    pub fn exit(&mut self) {
        self.active = false;
        self.selection = CopySelection::None;
    }

    fn max_col(&self) -> usize {
        self.viewport_cols.saturating_sub(1)
    }

    fn max_row(&self) -> i64 {
        (self.viewport_rows.saturating_sub(1)) as i64
    }

    fn sync_selection_head(&mut self) {
        match &mut self.selection {
            CopySelection::Linear { head, .. } | CopySelection::Block { head, .. } => {
                *head = self.cursor;
            }
            CopySelection::None => {}
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        }
        self.sync_selection_head();
    }

    pub fn move_right(&mut self) {
        if self.cursor.col < self.max_col() {
            self.cursor.col += 1;
        }
        self.sync_selection_head();
    }

    pub fn move_up(&mut self) {
        self.cursor.row = self.cursor.row.saturating_sub(1);
        self.sync_selection_head();
    }

    pub fn move_down(&mut self) {
        if self.cursor.row < self.max_row() {
            self.cursor.row += 1;
        }
        self.sync_selection_head();
    }

    pub fn move_word_forward(&mut self) {
        let max = self.max_col();
        if self.cursor.col >= max {
            // At end of line: advance to next row, col 0.
            if self.cursor.row < self.max_row() {
                self.cursor.row += 1;
                self.cursor.col = 0;
            }
        } else {
            self.cursor.col = (self.cursor.col + WORD_STEP).min(max);
        }
        self.sync_selection_head();
    }

    pub fn move_word_back(&mut self) {
        if self.cursor.col == 0 {
            // At line start: retreat to previous row, last col.
            self.cursor.row = self.cursor.row.saturating_sub(1);
            self.cursor.col = self.max_col();
        } else {
            self.cursor.col = self.cursor.col.saturating_sub(WORD_STEP);
        }
        self.sync_selection_head();
    }

    pub fn move_line_start(&mut self) {
        self.cursor.col = 0;
        self.sync_selection_head();
    }

    pub fn move_line_end(&mut self) {
        self.cursor.col = self.max_col();
        self.sync_selection_head();
    }

    pub fn page_up(&mut self) {
        let step = self.viewport_rows.max(1) as i64;
        self.cursor.row = self.cursor.row.saturating_sub(step);
        self.sync_selection_head();
    }

    pub fn page_down(&mut self) {
        let step = self.viewport_rows.max(1) as i64;
        self.cursor.row = (self.cursor.row + step).min(self.max_row());
        self.sync_selection_head();
    }

    pub fn start_selection_linear(&mut self) {
        self.selection = CopySelection::Linear {
            anchor: self.cursor,
            head: self.cursor,
        };
    }

    pub fn start_selection_block(&mut self) {
        self.selection = CopySelection::Block {
            anchor: self.cursor,
            head: self.cursor,
        };
    }

    pub fn cancel_selection(&mut self) {
        self.selection = CopySelection::None;
    }

    /// Returns `(start, end, is_block)` with endpoints normalised so that
    /// `start.row < end.row || (start.row == end.row && start.col <= end.col)`.
    pub fn normalised_selection(&self) -> Option<(CopyCursor, CopyCursor, bool)> {
        let (anchor, head, is_block) = match self.selection {
            CopySelection::None => return None,
            CopySelection::Linear { anchor, head } => (anchor, head, false),
            CopySelection::Block { anchor, head } => (anchor, head, true),
        };
        let swap = anchor.row > head.row
            || (anchor.row == head.row && anchor.col > head.col);
        let (start, end) = if swap { (head, anchor) } else { (anchor, head) };
        Some((start, end, is_block))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_sets_active_and_zero_cursor() {
        let mut cm = CopyMode::new();
        cm.enter(24, 80);
        assert!(cm.active);
        assert_eq!(cm.cursor, CopyCursor { row: 0, col: 0 });
        assert_eq!(cm.viewport_rows, 24);
        assert_eq!(cm.viewport_cols, 80);
        assert_eq!(cm.selection, CopySelection::None);
    }

    #[test]
    fn move_right_clamped_to_cols() {
        let mut cm = CopyMode::new();
        cm.enter(10, 5);
        for _ in 0..20 {
            cm.move_right();
        }
        assert_eq!(cm.cursor.col, 4);
    }

    #[test]
    fn move_down_clamped_to_visible() {
        let mut cm = CopyMode::new();
        cm.enter(3, 80);
        for _ in 0..10 {
            cm.move_down();
        }
        assert_eq!(cm.cursor.row, 2);
    }

    #[test]
    fn start_selection_then_move_extends_head() {
        let mut cm = CopyMode::new();
        cm.enter(10, 80);
        cm.start_selection_linear();
        cm.move_right();
        cm.move_right();
        cm.move_down();
        match cm.selection {
            CopySelection::Linear { anchor, head } => {
                assert_eq!(anchor, CopyCursor { row: 0, col: 0 });
                assert_eq!(head, CopyCursor { row: 1, col: 2 });
            }
            _ => panic!("expected Linear selection"),
        }
    }

    #[test]
    fn cancel_selection_clears() {
        let mut cm = CopyMode::new();
        cm.enter(10, 80);
        cm.start_selection_linear();
        cm.move_right();
        cm.cancel_selection();
        assert_eq!(cm.selection, CopySelection::None);
    }

    #[test]
    fn normalised_selection_swaps_when_anchor_after_head() {
        let mut cm = CopyMode::new();
        cm.enter(10, 80);
        cm.move_down();
        cm.move_down();
        cm.move_right();
        cm.move_right();
        cm.move_right();
        // Cursor at (2, 3). Start selection here, then move up-left.
        cm.start_selection_linear();
        cm.move_up();
        cm.move_left();
        // anchor = (2, 3), head = (1, 2) — should swap.
        let (start, end, is_block) = cm.normalised_selection().expect("selection");
        assert!(!is_block);
        assert_eq!(start, CopyCursor { row: 1, col: 2 });
        assert_eq!(end, CopyCursor { row: 2, col: 3 });
    }

    #[test]
    fn move_line_end_uses_viewport_cols() {
        let mut cm = CopyMode::new();
        cm.enter(10, 80);
        cm.move_line_end();
        assert_eq!(cm.cursor.col, 79);
    }

    #[test]
    fn block_selection_is_reported() {
        let mut cm = CopyMode::new();
        cm.enter(10, 80);
        cm.start_selection_block();
        cm.move_right();
        let (_, _, is_block) = cm.normalised_selection().expect("selection");
        assert!(is_block);
    }

    #[test]
    fn exit_clears_selection_and_active() {
        let mut cm = CopyMode::new();
        cm.enter(10, 80);
        cm.start_selection_linear();
        cm.exit();
        assert!(!cm.active);
        assert_eq!(cm.selection, CopySelection::None);
    }
}
