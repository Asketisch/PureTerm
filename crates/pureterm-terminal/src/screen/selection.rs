//! Mouse event handling and text selection for the terminal.

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
pub fn encode_mouse_x10(button: MouseButton, pressed: bool, row: usize, col: usize, _modifiers: MouseModifiers) -> Vec<u8> {
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
        assert_eq!(find_word_bounds(&line, 6), (6, 13)); // "world_foo"
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
