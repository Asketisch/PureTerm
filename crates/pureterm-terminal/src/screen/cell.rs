use crate::hyperlink::Hyperlink;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellAttrs {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub inverse: bool,
    pub hidden: bool,
    pub double_width: bool,
}

impl Default for CellAttrs {
    fn default() -> Self {
        Self {
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            strikethrough: false,
            inverse: false,
            hidden: false,
            double_width: false,
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamedColor {
    Foreground,
    Background,
    Cursor,
    SelectionBackground,
    SelectionForeground,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: CellAttrs,
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
            hyperlink: None,
        }
    }

    pub fn blank() -> Self {
        Self::new(' ')
    }

    pub fn is_blank(&self) -> bool {
        self.c == ' ' && self.fg == Color::Named(NamedColor::Foreground)
            && self.bg == Color::Named(NamedColor::Background)
            && self.hyperlink.is_none()
    }

    pub fn reset(&mut self) {
        self.c = ' ';
        self.fg = Color::Named(NamedColor::Foreground);
        self.bg = Color::Named(NamedColor::Background);
        self.attrs = CellAttrs::default();
        self.hyperlink = None;
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.c)
    }
}
