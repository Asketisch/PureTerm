//! GAP-17: Sixel / Kitty graphics protocol — parse + cache scaffold.

use std::collections::HashMap;

/// Parsed graphics placement (MVP: metadata only; GPU blit deferred).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphicsPlacement {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
    pub payload_len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct GraphicsCache {
    entries: HashMap<u32, GraphicsPlacement>,
    max_entries: usize,
}

impl GraphicsCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries: max_entries.max(1),
        }
    }

    /// Detect Kitty APC `\x1b_G` or Sixel DCS `\x1bPq` prefix.
    pub fn detect_protocol(chunk: &[u8]) -> Option<&'static str> {
        if chunk.starts_with(b"\x1b_G") {
            Some("kitty")
        } else if chunk.starts_with(b"\x1bPq") {
            Some("sixel")
        } else {
            None
        }
    }

    pub fn insert(&mut self, placement: GraphicsPlacement) {
        if self.entries.len() >= self.max_entries {
            if let Some(k) = self.entries.keys().next().copied() {
                self.entries.remove(&k);
            }
        }
        self.entries.insert(placement.id, placement);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalGraphicsConfig {
    pub enabled: bool,
    pub max_images: usize,
}

impl Default for TerminalGraphicsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_images: 64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_kitty_and_sixel() {
        assert_eq!(GraphicsCache::detect_protocol(b"\x1b_G"), Some("kitty"));
        assert_eq!(GraphicsCache::detect_protocol(b"\x1bPq"), Some("sixel"));
        assert!(GraphicsCache::detect_protocol(b"hello").is_none());
    }
}
