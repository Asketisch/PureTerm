//! Wgpu glyph atlas — bridges crossfont's RasterizedGlyph to a CPU-side texture atlas.
//!
//! Uses skyline bin-packing to place grayscale glyphs into a fixed 2048x2048 R8 bitmap.
//! When the atlas fills up, LRU entries are evicted and the atlas is rebuilt.
//! A `dirty` flag tracks when the bitmap needs re-uploading to the GPU.

use crossfont::{BitmapBuffer, RasterizedGlyph};
use std::collections::HashMap;

/// A cached glyph's location in the atlas.
#[derive(Clone, Copy, Debug)]
pub struct CachedGlyph {
    /// Top-left U coordinate in atlas UV space.
    pub u: f32,
    /// Top-left V coordinate in atlas UV space.
    pub v: f32,
    /// Bottom-right U coordinate in atlas UV space.
    pub u2: f32,
    /// Bottom-right V coordinate in atlas UV space.
    pub v2: f32,
    /// Horizontal offset from cell origin (pixels).
    pub x_offset: f32,
    /// Vertical offset from cell origin (pixels).
    pub y_offset: f32,
    /// Glyph width in pixels.
    pub width: f32,
    /// Glyph height in pixels.
    pub height: f32,
}

/// Skyline bin-packing segment.
#[derive(Clone, Debug)]
struct SkylineSeg {
    x: u32,
    y: u32,
    width: u32,
}

/// Atlas dimensions.
const ATLAS_SIZE: u32 = 2048;

/// Maximum number of cached glyphs before eviction kicks in.
const MAX_CACHE_ENTRIES: usize = 8192;

/// WgpuGlyphAtlas — CPU-side glyph cache with skyline bin packing.
///
/// Fixed 2048x2048 R8 (1 byte per pixel) bitmap = 4 MB.
/// When full, evicts LRU entries and rebuilds.
pub struct WgpuGlyphAtlas {
    atlas_width: u32,
    atlas_height: u32,
    /// R8 bitmap data (one byte per pixel).
    atlas_data: Vec<u8>,
    /// Skyline segments for bin packing.
    skyline: Vec<SkylineSeg>,
    /// Glyph cache: (font_index, character) -> atlas location.
    cache: HashMap<(usize, char), CachedGlyph>,
    /// Stored rasterized glyphs for rebuild.
    stored_glyphs: HashMap<(usize, char), RasterizedGlyph>,
    /// Simple LRU ordering: front = oldest, back = newest.
    lru_keys: Vec<(usize, char)>,
    /// Maximum cache entries.
    capacity: usize,
    /// Whether the atlas bitmap has changed since last GPU upload.
    dirty: bool,
}

impl WgpuGlyphAtlas {
    /// Create a new empty glyph atlas.
    pub fn new() -> Self {
        let mut skyline = Vec::with_capacity(64);
        skyline.push(SkylineSeg {
            x: 0,
            y: 0,
            width: ATLAS_SIZE,
        });

        Self {
            atlas_width: ATLAS_SIZE,
            atlas_height: ATLAS_SIZE,
            atlas_data: vec![0u8; (ATLAS_SIZE as usize) * (ATLAS_SIZE as usize)],
            skyline,
            cache: HashMap::new(),
            stored_glyphs: HashMap::new(),
            lru_keys: Vec::new(),
            capacity: MAX_CACHE_ENTRIES,
            dirty: false,
        }
    }

    /// Try to get a cached glyph. Returns `None` if not cached.
    /// On a cache hit, the key is moved to the back of the LRU list.
    pub fn get(&mut self, font_index: usize, c: char) -> Option<CachedGlyph> {
        let key = (font_index, c);
        if let Some(&glyph) = self.cache.get(&key) {
            // Move to end of LRU (most recently used).
            if let Some(pos) = self.lru_keys.iter().position(|k| *k == key) {
                self.lru_keys.remove(pos);
                self.lru_keys.push(key);
            }
            Some(glyph)
        } else {
            None
        }
    }

    /// Insert a rasterized glyph from crossfont into the atlas.
    ///
    /// Returns the `CachedGlyph` with UV coordinates. If the atlas is full,
    /// triggers a rebuild (evicts half the LRU entries).
    pub fn insert(&mut self, font_index: usize, c: char, glyph: &RasterizedGlyph) -> CachedGlyph {
        let key = (font_index, c);

        // If already cached, update LRU and return.
        if let Some(&cached) = self.cache.get(&key) {
            if let Some(pos) = self.lru_keys.iter().position(|k| *k == key) {
                self.lru_keys.remove(pos);
                self.lru_keys.push(key);
            }
            return cached;
        }

        // Extract grayscale bitmap from crossfont's RasterizedGlyph.
        let (glyph_width, glyph_height, grayscale) = Self::extract_grayscale(glyph);

        if glyph_width == 0 || glyph_height == 0 {
            // Degenerate glyph — return a zero-area cached entry.
            let cached = CachedGlyph {
                u: 0.0,
                v: 0.0,
                u2: 0.0,
                v2: 0.0,
                x_offset: glyph.left as f32,
                y_offset: glyph.top as f32,
                width: 0.0,
                height: 0.0,
            };
            self.cache.insert(key, cached);
            self.lru_keys.push(key);
            return cached;
        }

        // Try to allocate space. If it fails, rebuild the atlas.
        let pos = match self.allocate_rect(glyph_width, glyph_height) {
            Some(pos) => pos,
            None => {
                self.rebuild();
                self.allocate_rect(glyph_width, glyph_height).unwrap_or((0, 0))
            },
        };

        // Write glyph pixels into the atlas bitmap.
        self.blit_glyph(pos.0, pos.1, glyph_width, glyph_height, &grayscale);
        self.dirty = true;

        // Compute UV coordinates.
        let u = pos.0 as f32 / self.atlas_width as f32;
        let v = pos.1 as f32 / self.atlas_height as f32;
        let u2 = (pos.0 + glyph_width) as f32 / self.atlas_width as f32;
        let v2 = (pos.1 + glyph_height) as f32 / self.atlas_height as f32;

        let cached = CachedGlyph {
            u,
            v,
            u2,
            v2,
            x_offset: glyph.left as f32,
            y_offset: glyph.top as f32,
            width: glyph_width as f32,
            height: glyph_height as f32,
        };

        // Store the rasterized glyph for potential rebuild.
        self.stored_glyphs.insert(key, Self::clone_rasterized(glyph));
        self.cache.insert(key, cached);
        self.lru_keys.push(key);

        cached
    }

    /// Check if the atlas texture needs to be re-uploaded.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Get the atlas bitmap data and mark clean. Returns `(data, width, height)`.
    pub fn take_dirty_data(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        if self.dirty {
            self.dirty = false;
            Some((self.atlas_data.clone(), self.atlas_width, self.atlas_height))
        } else {
            None
        }
    }

    /// Get a reference to the atlas bitmap data for upload without marking clean.
    pub fn atlas_data(&self) -> &[u8] {
        &self.atlas_data
    }

    /// Atlas dimensions as `(width, height)`.
    pub fn atlas_dimensions(&self) -> (u32, u32) {
        (self.atlas_width, self.atlas_height)
    }

    // ---- Private helpers ----

    /// Extract a grayscale (R8) bitmap from a crossfont `RasterizedGlyph`.
    ///
    /// For `BitmapBuffer::Rgb`, converts RGB triplets to luminance.
    /// For `BitmapBuffer::Rgba`, converts RGBA to luminance (ignoring alpha).
    /// Returns `(width, height, grayscale_data)`.
    fn extract_grayscale(glyph: &RasterizedGlyph) -> (u32, u32, Vec<u8>) {
        let w = glyph.width as u32;
        let h = glyph.height as u32;
        if w == 0 || h == 0 {
            return (0, 0, Vec::new());
        }

        let gray = match &glyph.buffer {
            BitmapBuffer::Rgb(rgb) => {
                // RGB subpixel: convert to grayscale using luminance weights.
                rgb.chunks_exact(3)
                    .map(|px| {
                        let r = px[0] as u32;
                        let g = px[1] as u32;
                        let b = px[2] as u32;
                        // Luminance: average of RGB channels.
                        ((r + g + b) / 3) as u8
                    })
                    .collect()
            },
            BitmapBuffer::Rgba(rgba) => {
                // RGBA (colored emoji etc.): extract alpha as grayscale mask.
                rgba.chunks_exact(4).map(|px| px[3]).collect()
            },
        };

        (w, h, gray)
    }

    /// Clone a RasterizedGlyph for storage (needed for rebuild).
    fn clone_rasterized(glyph: &RasterizedGlyph) -> RasterizedGlyph {
        RasterizedGlyph {
            character: glyph.character,
            top: glyph.top,
            left: glyph.left,
            width: glyph.width,
            height: glyph.height,
            buffer: glyph.buffer.clone(),
            advance: glyph.advance,
        }
    }

    /// Blit a grayscale glyph into the atlas bitmap at `(x, y)`.
    fn blit_glyph(&mut self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        for row in 0..h {
            let dst_offset = ((y + row) * self.atlas_width + x) as usize;
            let src_offset = (row * w) as usize;
            let src_len = w as usize;
            if dst_offset + src_len <= self.atlas_data.len() && src_offset + src_len <= data.len() {
                self.atlas_data[dst_offset..dst_offset + src_len]
                    .copy_from_slice(&data[src_offset..src_offset + src_len]);
            }
        }
    }

    /// Allocate a rectangle in the atlas using skyline packing.
    ///
    /// Returns `(x, y)` top-left position or `None` if no space.
    fn allocate_rect(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width > self.atlas_width || height > self.atlas_height {
            return None;
        }

        let mut best_y = self.atlas_height;
        let mut best_idx = 0;
        let mut best_x = 0;

        // Find the skyline segment that gives the lowest Y for this width.
        for (i, _seg) in self.skyline.iter().enumerate() {
            let (x, y) = self.fit_width(i, width);
            if y + height < best_y {
                best_y = y + height;
                best_idx = i;
                best_x = x;
            }
        }

        // Check that the allocation fits within the atlas.
        if best_y > self.atlas_height {
            return None;
        }
        let top_y = best_y - height;

        // Add a new skyline segment for the allocated region.
        let new_seg = SkylineSeg {
            x: best_x,
            y: best_y,
            width,
        };

        // Merge / update skyline segments.
        self.skyline.insert(best_idx + 1, new_seg);

        // Remove covered segments and widen the one before if needed.
        self.merge_skyline();

        Some((best_x, top_y))
    }

    /// Find the minimum Y for placing `width` pixels starting at skyline index `idx`.
    fn fit_width(&self, idx: usize, width: u32) -> (u32, u32) {
        let seg = &self.skyline[idx];
        let mut x = seg.x;
        let mut remaining = width;
        let mut y = seg.y;
        let mut i = idx;

        while remaining > 0 {
            if i >= self.skyline.len() {
                return (x, self.atlas_height);
            }
            let s = &self.skyline[i];
            y = y.max(s.y);
            if y >= self.atlas_height {
                return (x, self.atlas_height);
            }
            if x + remaining <= s.x + s.width {
                return (x, y);
            }
            remaining -= s.x + s.width - x;
            x = s.x + s.width;
            i += 1;
        }

        (x, y)
    }

    /// Merge adjacent skyline segments that are at the same Y level.
    fn merge_skyline(&mut self) {
        let mut i = 1;
        while i < self.skyline.len() {
            let prev = &self.skyline[i - 1];
            let curr = &self.skyline[i];
            // If the previous segment extends past or to the same point, merge.
            if prev.x + prev.width >= curr.x && prev.y == curr.y {
                let end = (prev.x + prev.width).max(curr.x + curr.width);
                self.skyline[i - 1].width = end - prev.x;
                self.skyline.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Rebuild the atlas after eviction.
    ///
    /// Evicts the oldest half of the LRU entries and re-packs remaining glyphs.
    fn rebuild(&mut self) {
        // Determine how many entries to evict (half).
        let evict_count = self.lru_keys.len() / 2;
        if evict_count == 0 && self.lru_keys.len() > 0 {
            // If very small, just evict all and start fresh.
            self.cache.clear();
            self.stored_glyphs.clear();
            self.lru_keys.clear();
            self.skyline.clear();
            self.skyline.push(SkylineSeg {
                x: 0,
                y: 0,
                width: self.atlas_width,
            });
            self.atlas_data.fill(0);
            self.dirty = true;
            return;
        }

        // Collect keys to evict (oldest = front).
        let evict_keys: Vec<_> = self.lru_keys.drain(..evict_count).collect();
        for key in &evict_keys {
            self.cache.remove(key);
            self.stored_glyphs.remove(key);
        }

        // Reset atlas bitmap and skyline.
        self.atlas_data.fill(0);
        self.skyline.clear();
        self.skyline.push(SkylineSeg {
            x: 0,
            y: 0,
            width: self.atlas_width,
        });

        // Re-insert remaining glyphs. We need to clone the stored glyphs first
        // to avoid borrowing issues.
        let remaining: Vec<(usize, char, RasterizedGlyph)> = self
            .stored_glyphs
            .iter()
            .map(|((fi, c), g)| (*fi, *c, Self::clone_rasterized(g)))
            .collect();

        // Clear caches for re-insertion.
        self.cache.clear();
        self.stored_glyphs.clear();
        self.lru_keys.clear();

        for (font_index, c, glyph) in remaining {
            // Re-insert without triggering another rebuild.
            let (w, h, gray) = Self::extract_grayscale(&glyph);
            if w == 0 || h == 0 {
                continue;
            }
            if let Some((x, y)) = self.allocate_rect(w, h) {
                self.blit_glyph(x, y, w, h, &gray);
                let u = x as f32 / self.atlas_width as f32;
                let v = y as f32 / self.atlas_height as f32;
                let u2 = (x + w) as f32 / self.atlas_width as f32;
                let v2 = (y + h) as f32 / self.atlas_height as f32;

                let key = (font_index, c);
                let cached = CachedGlyph {
                    u,
                    v,
                    u2,
                    v2,
                    x_offset: glyph.left as f32,
                    y_offset: glyph.top as f32,
                    width: w as f32,
                    height: h as f32,
                };
                self.cache.insert(key, cached);
                self.stored_glyphs.insert(key, Self::clone_rasterized(&glyph));
                self.lru_keys.push(key);
            }
        }

        self.dirty = true;
    }
}

impl Default for WgpuGlyphAtlas {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_atlas_is_clean() {
        let atlas = WgpuGlyphAtlas::new();
        assert!(!atlas.is_dirty());
    }

    #[test]
    fn test_insert_marks_dirty() {
        let mut atlas = WgpuGlyphAtlas::new();
        let glyph = RasterizedGlyph {
            character: 'A',
            top: 0,
            left: 0,
            width: 10,
            height: 14,
            buffer: BitmapBuffer::Rgb(vec![128u8; 10 * 14 * 3]),
            advance: (10, 14),
        };
        let cached = atlas.insert(0, 'A', &glyph);
        assert!(atlas.is_dirty());
        assert_eq!(cached.width, 10.0);
        assert_eq!(cached.height, 14.0);
    }

    #[test]
    fn test_cache_hit() {
        let mut atlas = WgpuGlyphAtlas::new();
        let glyph = RasterizedGlyph {
            character: 'B',
            top: 0,
            left: 0,
            width: 8,
            height: 12,
            buffer: BitmapBuffer::Rgb(vec![200u8; 8 * 12 * 3]),
            advance: (8, 12),
        };
        atlas.insert(0, 'B', &glyph);
        assert!(atlas.get(0, 'B').is_some());
        assert!(atlas.get(0, 'C').is_none());
    }

    #[test]
    fn test_take_dirty_data_clears_flag() {
        let mut atlas = WgpuGlyphAtlas::new();
        let glyph = RasterizedGlyph {
            character: 'X',
            top: 0,
            left: 0,
            width: 6,
            height: 8,
            buffer: BitmapBuffer::Rgb(vec![100u8; 6 * 8 * 3]),
            advance: (6, 8),
        };
        atlas.insert(0, 'X', &glyph);
        let data = atlas.take_dirty_data();
        assert!(data.is_some());
        assert!(!atlas.is_dirty());
        // Second call should return None.
        assert!(atlas.take_dirty_data().is_none());
    }
}
