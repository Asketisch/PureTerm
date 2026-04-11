use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorStyle {
    Block,
    Underline,
    Beam,
}

impl Default for CursorStyle {
    fn default() -> Self {
        Self::Block
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub style: CursorStyle,
    pub blink: bool,
    pub visible: bool,
    pub saved_row: usize,
    pub saved_col: usize,
}

impl Cursor {
    pub fn new() -> Self {
        Self {
            row: 0,
            col: 0,
            style: CursorStyle::Block,
            blink: true,
            visible: true,
            saved_row: 0,
            saved_col: 0,
        }
    }

    pub fn save(&mut self) {
        self.saved_row = self.row;
        self.saved_col = self.col;
    }

    pub fn restore(&mut self) {
        self.row = self.saved_row;
        self.col = self.saved_col;
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}
