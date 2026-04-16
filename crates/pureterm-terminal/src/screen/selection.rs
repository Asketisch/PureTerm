//! Mouse event handling and text selection for the terminal.

use super::{Grid, WideMode};

/// A text selection within the terminal.
#[derive(Debug, Clone)]
pub struct Selection {
    /// Start row (0-based visible row).
    pub start_row: usize,
    /// Start column (0-based).
    pub start_col: usize,
    /// End row (0-based visible row).
    pub end_row: usize,
    /// End column (0-based).
    pub end_col: usize,
    /// Whether this is a rectangular (block) selection.
    pub rectangle: bool,
}

impl Selection {
    /// Create a new selection from two points.
    /// The points are NOT required to be in order.
    pub fn new(
        row1: usize, col1: usize,
        row2: usize, col2: usize,
        rectangle: bool,
    ) -> Self {
        Self {
            start_row: row1,
            start_col: col1,
            end_row: row2,
            end_col: col2,
            rectangle,
        }
    }

    /// Get the ordered (top-left, bottom-right) corners of the selection.
    pub fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.start_row < self.end_row
            || (self.start_row == self.end_row && self.start_col <= self.end_col)
        {
            ((self.start_row, self.start_col), (self.end_row, self.end_col))
        } else {
            ((self.end_row, self.end_col), (self.start_row, self.start_col))
        }
    }

    /// Check if a point is within the selection.
    pub fn contains(&self, row: usize, col: usize) -> bool {
        let ((tl_row, tl_col), (br_row, br_col)) = self.ordered();
        if self.rectangle {
            // Rectangle: check bounding box
            row >= tl_row && row <= br_row && col >= tl_col && col <= br_col
        } else {
            // Normal: full-line span between start and end
            if row < tl_row || row > br_row {
                return false;
            }
            if row == tl_row && row == br_row {
                col >= tl_col && col <= br_col
            } else if row == tl_row {
                col >= tl_col
            } else if row == br_row {
                col <= br_col
            } else {
                true
            }
        }
    }

    /// Extract the selected text from the grid.
    pub fn text(&self, grid: &Grid, cols: usize) -> String {
        let ((tl_row, tl_col), (br_row, br_col)) = self.ordered();
        let mut text = String::new();

        for row in tl_row..=br_row {
            if let Some(row_cells) = grid.visible_row(row) {
                let col_start = if row == tl_row { tl_col } else { 0 };
                let col_end = if row == br_row {
                    br_col.min(cols.saturating_sub(1))
                } else {
                    cols.saturating_sub(1)
                };

                if self.rectangle {
                    // Rectangle: extract only the column range on each row
                    for col in col_start..=col_end {
                        if col < row_cells.len() {
                            let cell = &row_cells[col];
                            if cell.wide != WideMode::SpacerTail
                                && cell.wide != WideMode::SpacerHead
                            {
                                text.push(cell.c);
                            }
                        }
                    }
                    if row < br_row {
                        text.push('\n');
                    }
                } else {
                    // Normal: extract full range, trim trailing spaces
                    let mut line_end = col_end;
                    // Find last non-space in range
                    for col in (col_start..=col_end).rev() {
                        if col < row_cells.len()
                            && row_cells[col].c != ' '
                            && row_cells[col].wide != WideMode::SpacerTail
                        {
                            line_end = col;
                            break;
                        }
                    }
                    for col in col_start..=line_end {
                        if col < row_cells.len() {
                            let cell = &row_cells[col];
                            if cell.wide != WideMode::SpacerTail
                                && cell.wide != WideMode::SpacerHead
                            {
                                text.push(cell.c);
                            }
                        }
                    }
                    if row < br_row {
                        text.push('\n');
                    }
                }
            }
        }
        text
    }

    /// Check if the selection is empty (start == end).
    pub fn is_empty(&self) -> bool {
        self.start_row == self.end_row && self.start_col == self.end_col
    }
}

/// Word boundary characters — used for double-click word selection.
const WORD_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-.@/";

/// Check if a character is a "word" character for selection purposes.
pub fn is_word_char(c: char) -> bool {
    WORD_CHARS.contains(c)
}

/// Find the boundaries of the word containing the given position.
/// Returns (start_col, end_col) inclusive.
pub fn find_word_bounds(line: &[char], col: usize) -> (usize, usize) {
    if col >= line.len() || line[col] == ' ' {
        return (col, col);
    }

    let is_word = is_word_char(line[col]);

    let mut start = col;
    while start > 0 && is_word_char(line[start - 1]) == is_word && line[start - 1] != ' ' {
        start -= 1;
    }

    let mut end = col;
    while end + 1 < line.len() && is_word_char(line[end + 1]) == is_word && line[end + 1] != ' ' {
        end += 1;
    }

    (start, end)
}

/// Mouse button for terminal events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// Mouse event to be processed by the terminal.
#[derive(Debug, Clone)]
pub enum TerminalMouseEvent {
    /// Mouse button pressed at (row, col).
    Down { button: MouseButton, row: usize, col: usize, modifiers: MouseModifiers },
    /// Mouse button released at (row, col).
    Up { button: MouseButton, row: usize, col: usize, modifiers: MouseModifiers },
    /// Mouse moved to (row, col).
    Move { row: usize, col: usize, modifiers: MouseModifiers },
    /// Mouse wheel scrolled.
    Scroll { delta_rows: i32, row: usize, col: usize, modifiers: MouseModifiers },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Click count for detecting double/triple clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickCount {
    Single,
    Double,
    Triple,
}

impl ClickCount {
    pub fn from_count(count: u8) -> Self {
        match count % 3 {
            0 => ClickCount::Triple,
            1 => ClickCount::Single,
            2 => ClickCount::Double,
            _ => ClickCount::Single,
        }
    }
}

/// Encode a mouse event as an SGR-style escape sequence.
pub fn encode_mouse_sgr(button: MouseButton, pressed: bool, row: usize, col: usize, modifiers: MouseModifiers) -> Vec<u8> {
    let cb = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let mut code = cb;
    if modifiers.shift { code += 4; }
    if modifiers.alt { code += 8; }
    if modifiers.ctrl { code += 16; }
    if !pressed { code += 32; } // motion/release

    let m = if pressed { 'M' } else { 'm' };
    format!("\x1b[<{};{};{}{}", code, col + 1, row + 1, m).into_bytes()
}

/// Encode a mouse event as X10-style escape sequence.
pub fn encode_mouse_x10(button: MouseButton, _pressed: bool, row: usize, col: usize, _modifiers: MouseModifiers) -> Vec<u8> {
    let cb = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    // X10 doesn't support modifiers, and col/row are encoded as byte + 32
    let cb_byte = (cb + 32) as u8;
    let col_byte = (col + 33).min(255) as u8;
    let row_byte = (row + 33).min(255) as u8;
    vec![0x1b, b'[', b'M', cb_byte, col_byte, row_byte]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_bounds() {
        let line: Vec<char> = "hello world_foo".chars().collect();
        assert_eq!(find_word_bounds(&line, 0), (0, 4)); // "hello"
        assert_eq!(find_word_bounds(&line, 6), (6, 14)); // "world_foo" (indices 6-14)
    }

    #[test]
    fn test_word_bounds_space() {
        let line: Vec<char> = "  space  ".chars().collect();
        assert_eq!(find_word_bounds(&line, 2), (2, 6)); // "space"
    }

    #[test]
    fn test_encode_mouse_sgr() {
        let encoded = encode_mouse_sgr(MouseButton::Left, true, 5, 10, MouseModifiers::default());
        let s = String::from_utf8(encoded).unwrap();
        assert_eq!(s, "\x1b[<0;11;6M");
    }

    #[test]
    fn test_click_count() {
        assert_eq!(ClickCount::from_count(1), ClickCount::Single);
        assert_eq!(ClickCount::from_count(2), ClickCount::Double);
        assert_eq!(ClickCount::from_count(3), ClickCount::Triple);
        assert_eq!(ClickCount::from_count(4), ClickCount::Single);
    }
}
