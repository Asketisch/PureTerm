use crate::screen::cell::{CellAttrs, Color, NamedColor, UnderlineStyle};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A style represents the visual attributes of a cell that can be deduplicated.
/// Multiple cells sharing the same style reference the same Style entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub attrs: CellAttrs,
}

impl Style {
    /// Create a new style with default values.
    pub fn new() -> Self {
        Self {
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            attrs: CellAttrs::default(),
        }
    }

    /// Get the foreground color.
    pub fn fg(&self) -> Color {
        self.fg
    }

    /// Get the background color.
    pub fn bg(&self) -> Color {
        self.bg
    }

    /// Get the cell attributes.
    pub fn attrs(&self) -> &CellAttrs {
        &self.attrs
    }

    /// Check if this is the default style (no special formatting).
    pub fn is_default(&self) -> bool {
        self.fg == Color::Named(NamedColor::Foreground)
            && self.bg == Color::Named(NamedColor::Background)
            && self.attrs == CellAttrs::default()
    }

    /// Create a style with the given foreground color.
    pub fn with_fg(fg: Color) -> Self {
        let mut style = Self::new();
        style.fg = fg;
        style
    }

    /// Create a style with the given background color.
    pub fn with_bg(bg: Color) -> Self {
        let mut style = Self::new();
        style.bg = bg;
        style
    }

    /// Create a style with bold enabled.
    pub fn with_bold(mut self) -> Self {
        self.attrs.bold = true;
        self
    }

    /// Create a style with italic enabled.
    pub fn with_italic(mut self) -> Self {
        self.attrs.italic = true;
        self
    }

    /// Create a style with underline.
    pub fn with_underline(mut self, style: UnderlineStyle) -> Self {
        self.attrs.underline = style;
        self
    }
}

impl Default for Style {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Style {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Style(fg={:?}, bg={:?}, attrs={:?})", self.fg, self.bg, self.attrs)
    }
}

/// A pool of styles with reference counting for deduplication.
#[derive(Debug, Clone)]
pub struct StylePool {
    styles: crate::screen::ref_counted_set::RefCountedSet<Style>,
    /// Cache of the default style index.
    default_style_index: Option<usize>,
}

impl StylePool {
    /// Create a new empty style pool.
    pub fn new() -> Self {
        Self {
            styles: crate::screen::ref_counted_set::RefCountedSet::new(),
            default_style_index: None,
        }
    }

    /// Get the index for the default style, creating it if necessary.
    pub fn default_style(&mut self) -> usize {
        if let Some(idx) = self.default_style_index {
            return idx;
        }
        let (idx, _) = self.styles.insert(Style::new());
        self.default_style_index = Some(idx);
        idx
    }

    /// Insert a style and get its stable index.
    /// Returns (index, is_new) where is_new indicates if this exact style was just added.
    pub fn insert(&mut self, style: Style) -> (usize, bool) {
        // Quick check for default style optimization
        if style.is_default() {
            return (self.default_style(), false);
        }
        self.styles.insert(style)
    }

    /// Remove one reference to a style by index.
    /// If the style's refcount reaches zero, it's removed from the pool.
    pub fn remove(&mut self, index: usize) {
        // First get the style, then remove it (to avoid borrow conflict)
        let style_to_remove = self.styles.get(index).cloned();
        if let Some(style) = style_to_remove {
            self.styles.remove(&style);
            // If we removed the default style, clear the cache
            if self.default_style_index == Some(index) {
                self.default_style_index = None;
            }
        }
    }

    /// Get a style by its index.
    pub fn get(&self, index: usize) -> Option<&Style> {
        self.styles.get(index)
    }

    /// Get the index for a style if it exists.
    pub fn get_index(&self, style: &Style) -> Option<usize> {
        self.styles.get_index(style)
    }

    /// Get the reference count for a style by index.
    pub fn ref_count(&self, index: usize) -> u32 {
        if let Some(style) = self.styles.get(index) {
            self.styles.ref_count(style)
        } else {
            0
        }
    }

    /// Get the number of unique styles in the pool.
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Check if the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// Clear all styles from the pool.
    pub fn clear(&mut self) {
        self.styles.clear();
        self.default_style_index = None;
    }
}

impl Default for StylePool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_style() {
        let mut pool = StylePool::new();
        let idx = pool.default_style();
        assert_eq!(idx, 0);

        // Inserting default style should return same index
        let (idx2, is_new) = pool.insert(Style::new());
        assert_eq!(idx, idx2);
        assert!(!is_new);
    }

    #[test]
    fn test_style_deduplication() {
        let mut pool = StylePool::new();
        let (idx1, _) = pool.insert(Style::new().with_bold());
        let (idx2, _) = pool.insert(Style::new().with_bold());
        assert_eq!(idx1, idx2);
    }

    #[test]
    fn test_remove_style() {
        let mut pool = StylePool::new();
        let (idx, _) = pool.insert(Style::new().with_bold());
        assert_eq!(pool.ref_count(idx), 1);
        pool.remove(idx);
        assert!(pool.get(idx).is_none());
    }
}
