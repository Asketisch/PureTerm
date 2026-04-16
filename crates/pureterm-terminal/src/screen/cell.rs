use crate::hyperlink::Hyperlink;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::{Hash, Hasher};

/// Wide character mode for a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WideMode {
    /// Normal single-width cell.
    Narrow,
    /// Left half of a 2-cell-wide character (contains the actual codepoint).
    Wide,
    /// Right half following a wide character (spacer, no display).
    SpacerTail,
    /// Spacer at end of a soft-wrapped line when a wide char wraps to next row.
    SpacerHead,
}

impl Default for WideMode {
    fn default() -> Self {
        WideMode::Narrow
    }
}

/// Underline style for text decoration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UnderlineStyle {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

impl Hash for UnderlineStyle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            UnderlineStyle::None => 0u8.hash(state),
            UnderlineStyle::Single => 1u8.hash(state),
            UnderlineStyle::Double => 2u8.hash(state),
            UnderlineStyle::Curly => 3u8.hash(state),
            UnderlineStyle::Dotted => 4u8.hash(state),
            UnderlineStyle::Dashed => 5u8.hash(state),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellAttrs {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: UnderlineStyle,
    pub underline_color: Color,
    pub strikethrough: bool,
    pub inverse: bool,
    pub hidden: bool,
    pub blink: bool,
    pub overline: bool,
}

impl Default for CellAttrs {
    fn default() -> Self {
        Self {
            bold: false,
            dim: false,
            italic: false,
            underline: UnderlineStyle::None,
            underline_color: Color::Named(NamedColor::Foreground),
            strikethrough: false,
            inverse: false,
            hidden: false,
            blink: false,
            overline: false,
        }
    }
}

impl Hash for CellAttrs {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bold.hash(state);
        self.dim.hash(state);
        self.italic.hash(state);
        self.underline.hash(state);
        self.underline_color.hash(state);
        self.strikethrough.hash(state);
        self.inverse.hash(state);
        self.hidden.hash(state);
        self.blink.hash(state);
        self.overline.hash(state);
        // Note: hyperlink is not included in hash since it's unique per cell
        // and not part of the visual style
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    Named(NamedColor),
    Palette(u8),
    Rgb(u8, u8, u8),
}

impl Default for Color {
    fn default() -> Self {
        Color::Named(NamedColor::Foreground)
    }
}

impl Hash for Color {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Color::Named(n) => {
                0u8.hash(state);
                n.hash(state);
            }
            Color::Palette(idx) => {
                1u8.hash(state);
                idx.hash(state);
            }
            Color::Rgb(r, g, b) => {
                2u8.hash(state);
                r.hash(state);
                g.hash(state);
                b.hash(state);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamedColor {
    Foreground,
    Background,
    Cursor,
    SelectionBackground,
    SelectionForeground,
}

impl Hash for NamedColor {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            NamedColor::Foreground => 0u8.hash(state),
            NamedColor::Background => 1u8.hash(state),
            NamedColor::Cursor => 2u8.hash(state),
            NamedColor::SelectionBackground => 3u8.hash(state),
            NamedColor::SelectionForeground => 4u8.hash(state),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: CellAttrs,
    /// Wide character mode.
    pub wide: WideMode,
    /// Whether this cell is protected from selective erase.
    pub protected: bool,
    /// Optional hyperlink (OSC 8). Stored as Arc for cheap cloning across cells.
    #[serde(
        default,
        skip_serializing,
        skip_deserializing
    )]
    pub hyperlink: Option<std::sync::Arc<Hyperlink>>,
}

impl Cell {
    pub fn new(c: char) -> Self {
        Self {
            c,
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            attrs: CellAttrs::default(),
            wide: WideMode::Narrow,
            protected: false,
            hyperlink: None,
        }
    }

    pub fn blank() -> Self {
        Self::new(' ')
    }

    pub fn is_blank(&self) -> bool {
        self.c == ' ' && self.fg == Color::Named(NamedColor::Foreground)
            && self.bg == Color::Named(NamedColor::Background)
            && self.wide == WideMode::Narrow
            && !self.protected
            && self.hyperlink.is_none()
    }

    pub fn reset(&mut self) {
        self.c = ' ';
        self.fg = Color::Named(NamedColor::Foreground);
        self.bg = Color::Named(NamedColor::Background);
        self.attrs = CellAttrs::default();
        self.wide = WideMode::Narrow;
        self.protected = false;
        self.hyperlink = None;
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.c)
    }
}
