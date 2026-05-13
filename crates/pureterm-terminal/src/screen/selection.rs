//! Mouse event handling and text selection for the terminal.

use once_cell::sync::Lazy;
use regex::Regex;

use super::{Grid, WideMode};

/// The kind of text selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionKind {
    /// Normal linear selection (start point to end point, spanning full rows).
    #[default]
    Linear,
    /// Rectangular/block selection (column-based, each row selects the same column range).
    Block,
}

impl SelectionKind {
    /// Returns true if this is a block selection.
    pub fn is_block(&self) -> bool {
        matches!(self, SelectionKind::Block)
    }

    /// Toggle between Linear and Block.
    pub fn toggle(&self) -> Self {
        match self {
            SelectionKind::Linear => SelectionKind::Block,
            SelectionKind::Block => SelectionKind::Linear,
        }
    }
}

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

    /// Get the selection kind.
    pub fn kind(&self) -> SelectionKind {
        if self.rectangle {
            SelectionKind::Block
        } else {
            SelectionKind::Linear
        }
    }

    /// Toggle between linear and block selection mode.
    pub fn toggle_kind(&mut self) {
        self.rectangle = !self.rectangle;
    }

    /// Set the selection kind.
    pub fn set_kind(&mut self, kind: SelectionKind) {
        self.rectangle = kind.is_block();
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

/// Smart selection patterns, compiled once and cached.
/// Ordered by specificity (highest precedence first). More specific patterns
/// must come before less specific ones so that, e.g., a URL wins over a bare
/// filepath when both match the same position.
///
/// Each entry is (regex, capture_group_index). For most patterns, capture group 0
/// (the full match) is used. For hex colors, we use a capture group to exclude
/// the leading non-alphanumeric boundary character.
static SMART_PATTERNS: Lazy<Vec<(Regex, usize)>> = Lazy::new(|| {
    fn re(pattern: &str) -> Regex {
        Regex::new(pattern).expect("smart selection regex malformed")
    }

    vec![
        // 1. URL with any scheme (http, https, ssh, ftp, git, file, hdfs, etc.)
        //    Matches: scheme://[user:pass@]host[:port][/path][?query][#fragment]
        //    Also handles IPv6 literals like http://[2001:db8::1]:80
        //    Host can be: domain with dots, bare hostname (no dots), or IPv6 literal
        (re(
            r"(?i)[a-z][a-z\d.+~-]*://([\w.-]+(:[\w.-]+)?@)?([\w-]+((\.[\w-]+)*)|(\[[\da-f:.]+\]))(:\d{1,5})?([\w.,@?^=%&:/~+#-]*[\w@?^=%&/~+#-])?"
        ), 0),

        // 2. Email address
        //    Matches: local-part@domain.tld
        //    Allows common email chars in local part: letters, digits, !#$%&'*+-/=?^`{|}~.
        (re(
            r"(?i)[\w\d!#$%&'*+\-/=?^`{|}~.]+@[a-z\d-]+\.[a-z\d.-]+[a-z\d-]"
        ), 0),

        // 3. Scientific notation floats (e.g., 6.02e+23, -8.1e1)
        (re(
            r"(?i)-?\d+(\.\d+)?e[+-]?\d+"
        ), 0),

        // 4. File path with optional :line or :line:col suffix
        //    Matches: /abs/path, ~/rel/path, ./rel/path, C:\win\path
        //    Also matches path with trailing :line or :line:col (e.g., src/main.rs:42:10)
        (re(
            r"(?i)(~|\b[a-z]:|[\w.*-]+)?[/\\][/\\\w.*-]+(?::\d+(?::\d+)?)?"
        ), 0),

        // 5. Hex color code (e.g., #ff0000, #RGB, #RRGGBBAA)
        //    Pattern uses a leading (?:^|[^a-zA-Z0-9]) to prevent matching inside words,
        //    and capture group 1 extracts just the #hex part.
        (re(
            r"(?:^|[^a-zA-Z0-9])(#[0-9a-fA-F]{3}\b|#[0-9a-fA-F]{4}\b|#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{8}\b)"
        ), 1),

        // 6. IPv4 address with optional port (e.g., 192.168.1.1, 10.0.0.1:8080)
        (re(
            r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d{1,5})?\b"
        ), 0),

        // 7. Identifier / hyphenated-name fallback
        //    Matches word chars connected by dots or hyphens (e.g., api-service-pod, foo.bar.baz)
        //    This also catches bare domain names, bare numbers with dots, etc.
        (re(
            r"(?i)\w+([.-]\w+)*"
        ), 0),
    ]
});

/// Smart selection: detect semantic patterns (URL, email, file path, etc.)
/// at the given column position. Returns (start_col, end_col) inclusive if a pattern matches.
///
/// Patterns are tried in specificity order. If multiple patterns match the click
/// position, the most specific one wins (e.g., full URL over bare filepath).
/// Falls back to None if no pattern covers the click position.
pub fn find_smart_selection(line: &[char], col: usize) -> Option<(usize, usize)> {
    if col >= line.len() || line[col] == ' ' {
        return None;
    }

    let line_str: String = line.iter().collect();

    for (regex, group) in SMART_PATTERNS.iter() {
        for caps in regex.captures_iter(&line_str) {
            if let Some(mat) = caps.get(*group) {
                let start = mat.start();
                let end = mat.end().saturating_sub(1); // inclusive
                if col >= start && col <= end {
                    return Some((start, end));
                }
            }
        }
    }

    None
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

    // --- Smart selection tests ---

    fn ss(line: &str, col: usize) -> Option<(usize, usize)> {
        let chars: Vec<char> = line.chars().collect();
        find_smart_selection(&chars, col)
    }

    // Extract selected text for easier assertions
    fn ss_text(line: &str, col: usize) -> Option<String> {
        ss(line, col).map(|(s, e)| line[s..=e].to_string())
    }

    #[test]
    fn test_smart_url_http() {
        assert_eq!(
            ss_text("visit http://stackoverflow.com now", 10),
            Some("http://stackoverflow.com".to_string())
        );
    }

    #[test]
    fn test_smart_url_https_full() {
        assert_eq!(
            ss_text("click https://andy:foo@stackoverflow.com/questions/123/how?foo=bar&baz=food#thing-here other", 35),
            Some("https://andy:foo@stackoverflow.com/questions/123/how?foo=bar&baz=food#thing-here".to_string())
        );
    }

    #[test]
    fn test_smart_url_ssh() {
        assert_eq!(
            ss_text("git clone ssh://git@github.com:acarl005/dotfiles.git", 10),
            Some("ssh://git@github.com:acarl005/dotfiles.git".to_string())
        );
    }

    #[test]
    fn test_smart_url_ftp() {
        assert_eq!(
            ss_text("send files here ftp://thing.example/file please", 16),
            Some("ftp://thing.example/file".to_string())
        );
    }

    #[test]
    fn test_smart_url_hdfs() {
        assert_eq!(
            ss_text("data hdfs://hadoopNS/data/users.csv there", 8),
            Some("hdfs://hadoopNS/data/users.csv".to_string())
        );
    }

    #[test]
    fn test_smart_email() {
        assert_eq!(
            ss_text("mail to: acarl005@g.ucla.edu thanks", 15),
            Some("acarl005@g.ucla.edu".to_string())
        );
        assert_eq!(
            ss_text("mail to: andy+1.hello@warp.dev thanks", 10),
            Some("andy+1.hello@warp.dev".to_string())
        );
    }

    #[test]
    fn test_smart_scientific_notation() {
        assert_eq!(
            ss_text("6.02e-23 -8.1e1 3.1415 8.1e+10-3.1415", 2),
            Some("6.02e-23".to_string())
        );
        assert_eq!(
            ss_text("6.02e-23 -8.1e1 3.1415 8.1e+10-3.1415", 11),
            Some("-8.1e1".to_string())
        );
        assert_eq!(
            ss_text("6.02e-23 -8.1e1 3.1415 8.1e+10-3.1415", 27),
            Some("8.1e+10".to_string())
        );
    }

    #[test]
    fn test_smart_filepath_unix() {
        assert_eq!(
            ss_text("~/.config/nvim/init.lua done", 2),
            Some("~/.config/nvim/init.lua".to_string())
        );
        assert_eq!(
            ss_text("scp ./foo.txt andy@ubuntu:/etc/foo.txt", 7),
            Some("./foo.txt".to_string())
        );
        assert_eq!(
            ss_text("scp ./foo.txt andy@ubuntu:/etc/foo.txt", 28),
            Some("/etc/foo.txt".to_string())
        );
    }

    #[test]
    fn test_smart_filepath_with_line_col() {
        assert_eq!(
            ss_text("error at src/main.rs:42:10 see log", 15),
            Some("src/main.rs:42:10".to_string())
        );
        assert_eq!(
            ss_text("see /home/user/code/file.rs:100", 8),
            Some("/home/user/code/file.rs:100".to_string())
        );
    }

    #[test]
    fn test_smart_hex_color() {
        assert_eq!(
            ss_text("color #ff0000 bold", 6),
            Some("#ff0000".to_string())
        );
        assert_eq!(
            ss_text("color #f00a alpha", 6),
            Some("#f00a".to_string())
        );
        assert_eq!(
            ss_text("color #1a2b3c4d long", 6),
            Some("#1a2b3c4d".to_string())
        );
    }

    #[test]
    fn test_smart_ipv4() {
        assert_eq!(
            ss_text("connect to 192.168.0.1 now", 11),
            Some("192.168.0.1".to_string())
        );
        assert_eq!(
            ss_text("server at 10.0.0.1:8080 ok", 10),
            Some("10.0.0.1:8080".to_string())
        );
    }

    #[test]
    fn test_smart_identifier_hyphenated() {
        assert_eq!(
            ss_text("api-service-pod running", 3),
            Some("api-service-pod".to_string())
        );
    }

    #[test]
    fn test_smart_identifier_dotted() {
        assert_eq!(
            ss_text("use std.collections.vector", 4),
            Some("std.collections.vector".to_string())
        );
    }

    #[test]
    fn test_smart_fallback_word() {
        // A simple word should still match via the identifier fallback
        assert_eq!(
            ss_text("hello world", 0),
            Some("hello".to_string())
        );
        assert_eq!(
            ss_text("hello world", 6),
            Some("world".to_string())
        );
    }

    #[test]
    fn test_smart_space_returns_none() {
        assert_eq!(ss("   ", 1), None);
    }

    #[test]
    fn test_smart_out_of_bounds() {
        let chars: Vec<char> = "hi".chars().collect();
        assert_eq!(find_smart_selection(&chars, 10), None);
    }

    #[test]
    fn test_smart_url_wins_over_filepath() {
        // Double-clicking on the path portion of a URL should select the whole URL
        assert_eq!(
            ss_text("see https://example.com/path/to/file.txt here", 30),
            Some("https://example.com/path/to/file.txt".to_string())
        );
    }

    #[test]
    fn test_smart_email_wins_over_identifier() {
        // The @ makes this an email, not two identifiers
        assert_eq!(
            ss_text("contact user.name+tag@example.co.uk please", 15),
            Some("user.name+tag@example.co.uk".to_string())
        );
    }
}
