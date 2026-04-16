//! Tmux layout string parser
//!
//! Parses tmux layout strings in the format: WxH,X,Y{...}[...]

use std::collections::HashMap;

/// A parsed pane
#[derive(Debug, Clone)]
pub struct PaneInfo {
    /// Pane ID
    pub id: String,
    /// X position
    pub x: u16,
    /// Y position
    pub y: u16,
    /// Width in cells
    pub width: u16,
    /// Height in cells
    pub height: u16,
    /// Whether this pane is active
    pub active: bool,
}

/// A split (horizontal or vertical)
#[derive(Debug, Clone)]
pub struct SplitInfo {
    /// Split type: "horizontal" or "vertical"
    pub split_type: SplitType,
    /// Child layouts
    pub children: Vec<Layout>,
    /// Sizes of each child
    pub sizes: Vec<u16>,
}

/// Layout types
#[derive(Debug, Clone)]
pub enum Layout {
    /// A leaf pane
    Pane(PaneInfo),
    /// A split container
    Split(SplitInfo),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitType {
    Horizontal,
    Vertical,
}

/// Layout parser errors
#[derive(Debug)]
pub enum LayoutError {
    /// Invalid format
    InvalidFormat(String),
    /// Unexpected end of input
    UnexpectedEnd,
    /// Invalid number
    InvalidNumber(String),
    /// Unclosed bracket
    UnclosedBracket,
    /// Unknown character
    UnknownChar(char),
}

/// Layout parser for tmux layout strings
pub struct LayoutParser {
    /// Input string
    input: String,
    /// Current position
    pos: usize,
    /// Pane ID to info mapping
    pane_info: HashMap<String, PaneInfo>,
}

impl LayoutParser {
    pub fn new(layout: &str) -> Self {
        Self {
            input: layout.to_string(),
            pos: 0,
            pane_info: HashMap::new(),
        }
    }

    /// Parse the layout string
    pub fn parse(&mut self) -> Result<Layout, LayoutError> {
        self.pos = 0;
        self.pane_info.clear();
        self.parse_layout()
    }

    /// Get pane info by ID
    pub fn get_pane(&self, id: &str) -> Option<&PaneInfo> {
        self.pane_info.get(id)
    }

    /// Get all panes
    pub fn all_panes(&self) -> Vec<&PaneInfo> {
        self.pane_info.values().collect()
    }

    fn parse_layout(&mut self) -> Result<Layout, LayoutError> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Err(LayoutError::UnexpectedEnd);
        }

        let ch = self.peek();

        if ch == Some('{') {
            self.parse_split()
        } else if ch == Some('[') {
            self.parse_split()
        } else if ch.is_some() && ch.unwrap().is_ascii_digit() {
            // Could be pane or split
            self.parse_maybe_split()
        } else if ch.is_some() {
            Err(LayoutError::UnknownChar(ch.unwrap()))
        } else {
            Err(LayoutError::UnexpectedEnd)
        }
    }

    fn parse_maybe_split(&mut self) -> Result<Layout, LayoutError> {
        // Look ahead to see if this is just a pane or the start of a split
        let start_pos = self.pos;

        // Try to parse as: WxH,X,Y or WxH,X,Y{...} or WxH,X,Y[...]
        let size = self.parse_size_xy()?;
        self.skip_whitespace();

        let ch = self.peek();

        if ch == Some('{') {
            // It's a split
            self.pos = start_pos;
            self.parse_split()
        } else if ch == Some('[') {
            // Could be a split or a pane with ID suffix like [%id]
            // Look ahead to distinguish: if we see [%id] it's a pane, else it's a split
            let saved_pos = self.pos;
            self.next(); // consume '['
            self.skip_whitespace();
            if self.peek() == Some('%') {
                // This is a pane with ID suffix like [%id*]
                self.pos = saved_pos;
                self.parse_pane(size)
            } else {
                // It's a split
                self.pos = saved_pos;
                self.parse_split()
            }
        } else {
            // It's a pane
            self.parse_pane(size)
        }
    }

    fn parse_split(&mut self) -> Result<Layout, LayoutError> {
        let _start = self.pos;
        let ch = self.next().ok_or(LayoutError::UnexpectedEnd)?;

        let split_type = if ch == '{' {
            SplitType::Horizontal
        } else if ch == '[' {
            SplitType::Vertical
        } else {
            return Err(LayoutError::UnknownChar(ch));
        };

        let mut children = Vec::new();
        let mut sizes = Vec::new();

        loop {
            self.skip_whitespace();

            if self.pos >= self.input.len() {
                return Err(LayoutError::UnclosedBracket);
            }

            let ch = self.peek();

            if ch == Some('}') || ch == Some(']') {
                // Closing bracket found, collect sizes after it
                self.next();
                self.skip_whitespace();
                while let Ok(size) = self.parse_number() {
                    sizes.push(size);
                    self.skip_whitespace();
                }
                break;
            }

            // If we're not at a closing bracket and not at a child start,
            // we might be at a separator (,) - skip it
            if ch == Some(',') {
                self.next();
                continue;
            }

            // Parse child layout
            let child = self.parse_layout()?;
            children.push(child);
        }

        Ok(Layout::Split(SplitInfo {
            split_type,
            children,
            sizes,
        }))
    }

    fn parse_pane(&mut self, size: (u16, u16, u16, u16)) -> Result<Layout, LayoutError> {
        let (width, height, x, y) = size;

        self.skip_whitespace();

        // Parse optional pane ID in brackets: [%id*]
        let (id, active) = if self.peek() == Some('[') {
            self.next(); // consume '['
            self.skip_whitespace();

            // Parse optional %
            let id = if self.peek() == Some('%') {
                self.next();
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        self.next();
                    } else {
                        break;
                    }
                }
                let id = self.input[start..self.pos].to_string();
                if id.is_empty() {
                    return Err(LayoutError::InvalidFormat("empty pane id".to_string()));
                }
                id
            } else {
                format!("p{}", self.pane_info.len())
            };

            self.skip_whitespace();

            // Parse active flag
            let active = if self.peek() == Some('*') {
                self.next();
                true
            } else {
                false
            };

            self.skip_whitespace();

            // Expect closing ]
            if self.peek() != Some(']') {
                return Err(LayoutError::InvalidFormat("expected ']'".to_string()));
            }
            self.next(); // consume ']'

            (id, active)
        } else {
            // No brackets, generate default ID
            (format!("p{}", self.pane_info.len()), false)
        };

        let pane = PaneInfo {
            id: id.clone(),
            x,
            y,
            width,
            height,
            active,
        };

        self.pane_info.insert(id, pane.clone());

        Ok(Layout::Pane(pane))
    }

    fn parse_size_xy(&mut self) -> Result<(u16, u16, u16, u16), LayoutError> {
        // Format: WxH,X,Y or WxH,X,Y{...} or WxH,X,Y[...]
        let _start = self.pos;

        // Parse W
        let w = self.parse_number()?;

        if self.peek() != Some('x') {
            return Err(LayoutError::InvalidFormat("expected 'x'".to_string()));
        }
        self.next();

        // Parse H
        let h = self.parse_number()?;

        if self.peek() != Some(',') {
            return Err(LayoutError::InvalidFormat("expected ','".to_string()));
        }
        self.next();

        // Parse X
        let x = self.parse_number()?;

        if self.peek() != Some(',') {
            return Err(LayoutError::InvalidFormat("expected ','".to_string()));
        }
        self.next();

        // Parse Y
        let y = self.parse_number()?;

        Ok((w, h, x, y))
    }

    fn parse_number(&mut self) -> Result<u16, LayoutError> {
        let start = self.pos;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.next();
            } else {
                break;
            }
        }

        if start == self.pos {
            return Err(LayoutError::InvalidNumber("empty number".to_string()));
        }

        let num_str = &self.input[start..self.pos];
        num_str.parse().map_err(|_| LayoutError::InvalidNumber(num_str.to_string()))
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.input[self.pos..].chars().next()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' {
                self.next();
            } else {
                break;
            }
        }
    }
}

/// Validate a layout string
pub fn validate(layout: &str) -> Result<Layout, LayoutError> {
    let mut parser = LayoutParser::new(layout);
    parser.parse()
}

/// Flatten a layout into all panes
pub fn flatten(layout: &Layout) -> Vec<&PaneInfo> {
    match layout {
        Layout::Pane(pane) => vec![pane],
        Layout::Split(split) => {
            let mut panes = Vec::new();
            for child in &split.children {
                panes.extend(flatten(child));
            }
            panes
        }
    }
}

/// Calculate the bounds of a layout
pub fn bounds(layout: &Layout) -> (u16, u16) {
    match layout {
        Layout::Pane(pane) => (pane.x + pane.width, pane.y + pane.height),
        Layout::Split(split) => {
            let mut max_x = 0u16;
            let mut max_y = 0u16;
            for child in &split.children {
                let (cx, cy) = bounds(child);
                max_x = max_x.max(cx);
                max_y = max_y.max(cy);
            }
            (max_x, max_y)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_pane() {
        // tmux layout: WxH,X,Y
        let layout = "80x24,0,0";
        let result = validate(layout);
        assert!(result.is_ok());
        match result.unwrap() {
            Layout::Pane(pane) => {
                assert_eq!(pane.width, 80);
                assert_eq!(pane.height, 24);
                assert_eq!(pane.x, 0);
                assert_eq!(pane.y, 0);
            }
            _ => panic!("Expected Pane"),
        }
    }

    #[test]
    fn test_parse_pane_with_id() {
        // A pane with ID inside a split: {WxH,X,Y[%id]}
        // Note: standalone panes don't have ID syntax in tmux
        let layout = "{80x24,0,0[%0]}0,0";
        let result = validate(layout);
        assert!(result.is_ok());
        match result.unwrap() {
            Layout::Split(split) => {
                assert_eq!(split.children.len(), 1);
            }
            _ => panic!("Expected Split"),
        }
    }

    #[test]
    fn test_parse_active_pane() {
        // Active pane inside a split
        let layout = "{80x24,0,0[%0*]}0,0";
        let result = validate(layout);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_horizontal_split() {
        // tmux horizontal split: {left,right}sizes
        // Format: {child1,child2,...}sizes
        let layout = "{80x24,0,0[%0],80x24,80,0[%1]}0,0";
        let result = validate(layout);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_vertical_split() {
        // tmux vertical split: [top,bottom]sizes
        // Format: [child1,child2,...]sizes
        let layout = "[80x24,0,0[%0],80x24,0,24[%1]]0,0";
        let result = validate(layout);
        assert!(result.is_ok());
    }

    #[test]
    fn test_flatten() {
        // Horizontal split with two panes
        let layout = "{80x24,0,0[%0],80x24,80,0[%1]}0,0";
        let result = validate(layout).unwrap();
        let panes = flatten(&result);
        assert_eq!(panes.len(), 2);
    }

    #[test]
    fn test_bounds() {
        let layout = "80x24,0,0";
        let result = validate(layout).unwrap();
        let (w, h) = bounds(&result);
        assert_eq!(w, 80);
        assert_eq!(h, 24);
    }
}
