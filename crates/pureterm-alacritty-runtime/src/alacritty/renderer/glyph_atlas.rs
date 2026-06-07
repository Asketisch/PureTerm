//! Wgpu glyph atlas — bridges crossfont's RasterizedGlyph to a CPU-side texture atlas.
//!
//! Dual-atlas design: a grayscale R8 atlas for text glyphs and an RGBA atlas for
//! color emoji. Skyline bin-packing places glyphs into fixed 2048x2048 bitmaps.
//! When an atlas fills up, LRU entries are evicted and both atlases are rebuilt.

use crossfont::{BitmapBuffer, RasterizedGlyph};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

/// Which atlas a glyph lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasKind {
    Grayscale,
    Color,
}

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
    /// Which atlas contains this glyph.
    pub atlas_kind: AtlasKind,
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

/// A rectangular region of the atlas that changed since the last upload.
///
/// Used by the render thread to do partial `queue.write_texture` calls
/// instead of re-uploading the entire 4MB+16MB atlas on every frame that
/// touched a single glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// What the render thread needs to upload changed atlas regions.
///
/// `data` is the full atlas (one contiguous `Arc<Vec<u8>>`); the render
/// thread slices the sub-rectangles with byte offsets in
/// `TexelCopyBufferLayout`. Returning `Arc` lets the main thread and
/// render thread share the allocation without a 20MB clone.
pub struct AtlasDirty {
    pub regions: Vec<DirtyRect>,
    pub data: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// WgpuGlyphAtlas — CPU-side glyph cache with skyline bin packing.
///
/// Dual-atlas: grayscale R8 (4 MB) for text, RGBA (16 MB) for color emoji.
/// When full, evicts LRU entries and rebuilds.
pub struct WgpuGlyphAtlas {
    atlas_width: u32,
    atlas_height: u32,
    /// R8 bitmap data (one byte per pixel).
    atlas_data: Vec<u8>,
    /// Skyline segments for grayscale bin packing.
    skyline: Vec<SkylineSeg>,
    /// Glyph cache: (font_index, character) -> atlas location.
    cache: HashMap<(usize, char), CachedGlyph>,
    /// Stored rasterized glyphs for rebuild.
    stored_glyphs: HashMap<(usize, char), RasterizedGlyph>,
    /// LRU ordering backed by `IndexMap`: front = oldest, back = newest.
    /// PureTerm: O(1) `shift_remove_index(0)` for eviction (was O(n)
    /// `Vec::position` + `Vec::remove` before).
    lru_keys: IndexMap<(usize, char), ()>,
    /// Maximum cache entries.
    capacity: usize,
    /// Whether the grayscale atlas bitmap has changed since last GPU upload.
    dirty: bool,
    /// Per-rect tracking of grayscale changes since last `take_dirty_regions`.
    /// Used by the render thread to do partial uploads.
    dirty_regions: Vec<DirtyRect>,
    /// RGBA color atlas bitmap (4 bytes per pixel).
    color_atlas_data: Vec<u8>,
    /// Skyline segments for color atlas bin packing.
    color_skyline: Vec<SkylineSeg>,
    /// Whether the color atlas bitmap has changed since last GPU upload.
    color_dirty: bool,
    /// Per-rect tracking of color atlas changes since last
    /// `take_color_dirty_regions`.
    color_dirty_regions: Vec<DirtyRect>,
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
        let mut color_skyline = Vec::with_capacity(64);
        color_skyline.push(SkylineSeg {
            x: 0,
            y: 0,
            width: ATLAS_SIZE,
        });

        Self {
            atlas_width: ATLAS_SIZE,
            atlas_height: ATLAS_SIZE,
            atlas_data: vec![0u8; (ATLAS_SIZE as usize) * (ATLAS_SIZE as usize)],
            skyline,
            // PureTerm: 预分配到 MAX_CACHE_ENTRIES 避免启动期 rehash。
            cache: HashMap::with_capacity(MAX_CACHE_ENTRIES),
            stored_glyphs: HashMap::with_capacity(MAX_CACHE_ENTRIES),
            lru_keys: IndexMap::with_capacity(MAX_CACHE_ENTRIES),
            capacity: MAX_CACHE_ENTRIES,
            dirty: false,
            dirty_regions: Vec::with_capacity(64),
            color_atlas_data: vec![0u8; (ATLAS_SIZE as usize) * (ATLAS_SIZE as usize) * 4],
            color_skyline,
            color_dirty: false,
            color_dirty_regions: Vec::with_capacity(64),
        }
    }

    /// Try to get a cached glyph. Returns `None` if not cached.
    /// On a cache hit, the key is moved to the back of the LRU list.
    pub fn get(&mut self, font_index: usize, c: char) -> Option<CachedGlyph> {
        let key = (font_index, c);
        if let Some(&glyph) = self.cache.get(&key) {
            // Move to end of LRU (most recently used). O(1) with IndexMap.
            self.lru_keys.shift_remove(&key);
            self.lru_keys.insert(key, ());
            Some(glyph)
        } else {
            None
        }
    }

    /// Insert a rasterized glyph from crossfont into the atlas.
    ///
    /// Routes `BitmapBuffer::Rgba` glyphs to the color atlas and all others
    /// to the grayscale atlas. Returns the `CachedGlyph` with UV coordinates.
    /// If the atlas is full, triggers a rebuild (evicts half the LRU entries).
    pub fn insert(&mut self, font_index: usize, c: char, glyph: &RasterizedGlyph) -> CachedGlyph {
        let key = (font_index, c);

        // If already cached, update LRU and return. O(1) with IndexMap.
        if let Some(&cached) = self.cache.get(&key) {
            self.lru_keys.shift_remove(&key);
            self.lru_keys.insert(key, ());
            return cached;
        }

        let is_color = matches!(&glyph.buffer, BitmapBuffer::Rgba(_));

        let cached = if is_color {
            self.insert_color(font_index, c, glyph)
        } else {
            self.insert_grayscale(font_index, c, glyph)
        };

        // Store the rasterized glyph for potential rebuild.
        self.stored_glyphs.insert(key, Self::clone_rasterized(glyph));
        self.cache.insert(key, cached);
        self.lru_keys.insert(key, ());

        cached
    }

    /// Insert a grayscale glyph into the R8 atlas.
    fn insert_grayscale(
        &mut self,
        font_index: usize,
        c: char,
        glyph: &RasterizedGlyph,
    ) -> CachedGlyph {
        let (glyph_width, glyph_height, grayscale) = Self::extract_grayscale(glyph);

        if glyph_width == 0 || glyph_height == 0 {
            return CachedGlyph {
                u: 0.0,
                v: 0.0,
                u2: 0.0,
                v2: 0.0,
                x_offset: glyph.left as f32,
                y_offset: glyph.top as f32,
                width: 0.0,
                height: 0.0,
                atlas_kind: AtlasKind::Grayscale,
            };
        }

        let pos = match self.allocate_rect(glyph_width, glyph_height) {
            Some(pos) => pos,
            None => {
                self.rebuild();
                self.allocate_rect(glyph_width, glyph_height).unwrap_or((0, 0))
            },
        };

        self.blit_glyph(pos.0, pos.1, glyph_width, glyph_height, &grayscale);
        self.dirty = true;
        self.dirty_regions.push(DirtyRect {
            x: pos.0, y: pos.1, w: glyph_width, h: glyph_height,
        });

        let u = pos.0 as f32 / self.atlas_width as f32;
        let v = pos.1 as f32 / self.atlas_height as f32;
        let u2 = (pos.0 + glyph_width) as f32 / self.atlas_width as f32;
        let v2 = (pos.1 + glyph_height) as f32 / self.atlas_height as f32;

        CachedGlyph {
            u,
            v,
            u2,
            v2,
            x_offset: glyph.left as f32,
            y_offset: glyph.top as f32,
            width: glyph_width as f32,
            height: glyph_height as f32,
            atlas_kind: AtlasKind::Grayscale,
        }
    }

    /// Insert a color (RGBA) glyph into the color atlas.
    fn insert_color(
        &mut self,
        _font_index: usize,
        _c: char,
        glyph: &RasterizedGlyph,
    ) -> CachedGlyph {
        let (glyph_width, glyph_height, rgba) = Self::extract_rgba(glyph);

        if glyph_width == 0 || glyph_height == 0 {
            return CachedGlyph {
                u: 0.0,
                v: 0.0,
                u2: 0.0,
                v2: 0.0,
                x_offset: glyph.left as f32,
                y_offset: glyph.top as f32,
                width: 0.0,
                height: 0.0,
                atlas_kind: AtlasKind::Color,
            };
        }

        let pos = match self.allocate_color_rect(glyph_width, glyph_height) {
            Some(pos) => pos,
            None => {
                self.rebuild();
                self.allocate_color_rect(glyph_width, glyph_height).unwrap_or((0, 0))
            },
        };

        self.blit_color_glyph(pos.0, pos.1, glyph_width, glyph_height, &rgba);
        self.color_dirty = true;
        self.color_dirty_regions.push(DirtyRect {
            x: pos.0, y: pos.1, w: glyph_width, h: glyph_height,
        });

        let u = pos.0 as f32 / self.atlas_width as f32;
        let v = pos.1 as f32 / self.atlas_height as f32;
        let u2 = (pos.0 + glyph_width) as f32 / self.atlas_width as f32;
        let v2 = (pos.1 + glyph_height) as f32 / self.atlas_height as f32;

        CachedGlyph {
            u,
            v,
            u2,
            v2,
            x_offset: glyph.left as f32,
            y_offset: glyph.top as f32,
            width: glyph_width as f32,
            height: glyph_height as f32,
            atlas_kind: AtlasKind::Color,
        }
    }

    /// Check if the atlas texture needs to be re-uploaded.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Force both grayscale and color atlas textures to be re-uploaded on next render.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.color_dirty = true;
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

    /// Get the dirty regions + the full atlas buffer, marking the atlas
    /// clean. Returns `None` if the atlas is clean.
    ///
    /// `data` is an `Arc<Vec<u8>>` so the main thread and the render
    /// thread can hold the buffer without an extra clone. The full atlas
    /// is returned (not just the dirty sub-rectangles) because the GPU
    /// upload API wants a `&[u8]` slice with a row stride; the render
    /// thread uses each region's `(x, y)` as the `TexelCopyTextureInfo`
    /// origin and a byte offset into this buffer.
    pub fn take_dirty_regions(&mut self) -> Option<AtlasDirty> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        let regions = std::mem::take(&mut self.dirty_regions);
        Some(AtlasDirty {
            regions,
            data: Arc::new(self.atlas_data.clone()),
            width: self.atlas_width,
            height: self.atlas_height,
        })
    }

    /// Get a reference to the atlas bitmap data for upload without marking clean.
    pub fn atlas_data(&self) -> &[u8] {
        &self.atlas_data
    }

    /// Atlas dimensions as `(width, height)`.
    pub fn atlas_dimensions(&self) -> (u32, u32) {
        (self.atlas_width, self.atlas_height)
    }

    /// Check if the color atlas texture needs to be re-uploaded.
    pub fn is_color_dirty(&self) -> bool {
        self.color_dirty
    }

    /// Get the color atlas bitmap data and mark clean. Returns `(data, width, height)`.
    pub fn take_color_dirty_data(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        if self.color_dirty {
            self.color_dirty = false;
            Some((self.color_atlas_data.clone(), self.atlas_width, self.atlas_height))
        } else {
            None
        }
    }

    /// Color-atlas counterpart of [`take_dirty_regions`].
    pub fn take_color_dirty_regions(&mut self) -> Option<AtlasDirty> {
        if !self.color_dirty {
            return None;
        }
        self.color_dirty = false;
        let regions = std::mem::take(&mut self.color_dirty_regions);
        Some(AtlasDirty {
            regions,
            data: Arc::new(self.color_atlas_data.clone()),
            width: self.atlas_width,
            height: self.atlas_height,
        })
    }

    // ---- Private helpers ----

    /// Extract a grayscale (R8) bitmap from a crossfont `RasterizedGlyph`.
    ///
    /// Only handles `BitmapBuffer::Rgb`. Returns `(width, height, grayscale_data)`.
    fn extract_grayscale(glyph: &RasterizedGlyph) -> (u32, u32, Vec<u8>) {
        let w = glyph.width as u32;
        let h = glyph.height as u32;
        if w == 0 || h == 0 {
            return (0, 0, Vec::new());
        }

        let gray = match &glyph.buffer {
            BitmapBuffer::Rgb(rgb) => {
                rgb.chunks_exact(3)
                    .map(|px| {
                        let r = px[0] as u32;
                        let g = px[1] as u32;
                        let b = px[2] as u32;
                        ((r + g + b) / 3) as u8
                    })
                    .collect()
            },
            BitmapBuffer::Rgba(rgba) => {
                // Fallback: extract alpha as grayscale mask.
                rgba.chunks_exact(4).map(|px| px[3]).collect()
            },
        };

        (w, h, gray)
    }

    /// Extract RGBA pixel data from a color glyph.
    fn extract_rgba(glyph: &RasterizedGlyph) -> (u32, u32, Vec<u8>) {
        let w = glyph.width as u32;
        let h = glyph.height as u32;
        if w == 0 || h == 0 {
            return (0, 0, Vec::new());
        }

        let rgba = match &glyph.buffer {
            BitmapBuffer::Rgba(data) => data.clone(),
            BitmapBuffer::Rgb(_) => Vec::new(),
        };

        (w, h, rgba)
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

    /// Blit an RGBA color glyph into the color atlas at `(x, y)`.
    fn blit_color_glyph(&mut self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        let row_bytes = w as usize * 4;
        for row in 0..h {
            let dst_offset = ((y + row) * self.atlas_width + x) as usize * 4;
            let src_offset = (row as usize) * row_bytes;
            if dst_offset + row_bytes <= self.color_atlas_data.len()
                && src_offset + row_bytes <= data.len()
            {
                self.color_atlas_data[dst_offset..dst_offset + row_bytes]
                    .copy_from_slice(&data[src_offset..src_offset + row_bytes]);
            }
        }
    }

    /// Allocate a rectangle in the color atlas using skyline bin-packing.
    fn allocate_color_rect(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width == 0 || height == 0 || width > self.atlas_width || height > self.atlas_height {
            return None;
        }

        let mut best_y = u32::MAX;
        let mut best_idx = 0;

        for i in 0..self.color_skyline.len() {
            if let Some(y) = self.fit_color_at(i, width, height) {
                if y + height < best_y {
                    best_y = y + height;
                    best_idx = i;
                }
            }
        }

        if best_y == u32::MAX {
            return None;
        }

        let top_y = best_y - height;
        let best_x = self.color_skyline[best_idx].x;

        let new_seg = SkylineSeg {
            x: best_x,
            y: best_y,
            width,
        };
        self.color_skyline.insert(best_idx, new_seg);

        let right_edge = best_x + width;
        let mut i = best_idx + 1;
        while i < self.color_skyline.len() {
            let seg_end = self.color_skyline[i].x + self.color_skyline[i].width;
            if self.color_skyline[i].x < right_edge {
                if seg_end <= right_edge {
                    self.color_skyline.remove(i);
                } else {
                    let shrink = right_edge - self.color_skyline[i].x;
                    self.color_skyline[i].x += shrink;
                    self.color_skyline[i].width -= shrink;
                    break;
                }
            } else {
                break;
            }
        }

        self.merge_color_skyline();

        Some((best_x, top_y))
    }

    fn fit_color_at(&self, idx: usize, width: u32, height: u32) -> Option<u32> {
        let start_x = self.color_skyline[idx].x;
        let right_edge = start_x + width;
        if right_edge > self.atlas_width {
            return None;
        }

        let mut max_y = 0u32;
        let mut i = idx;
        while i < self.color_skyline.len() && self.color_skyline[i].x < right_edge {
            max_y = max_y.max(self.color_skyline[i].y);
            if max_y + height > self.atlas_height {
                return None;
            }
            i += 1;
        }
        Some(max_y)
    }

    fn merge_color_skyline(&mut self) {
        let mut i = 0;
        while i + 1 < self.color_skyline.len() {
            if self.color_skyline[i].y == self.color_skyline[i + 1].y {
                self.color_skyline[i].width += self.color_skyline[i + 1].width;
                self.color_skyline.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }

    /// Allocate a rectangle in the atlas using skyline bin-packing.
    ///
    /// Returns `(x, y)` top-left position or `None` if no space.
    fn allocate_rect(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width == 0 || height == 0 || width > self.atlas_width || height > self.atlas_height {
            return None;
        }

        let mut best_y = u32::MAX;
        let mut best_idx = 0;

        // Find the skyline position that gives the lowest resulting Y.
        for i in 0..self.skyline.len() {
            if let Some(y) = self.fit_at(i, width, height) {
                if y + height < best_y {
                    best_y = y + height;
                    best_idx = i;
                }
            }
        }

        if best_y == u32::MAX {
            return None;
        }

        let top_y = best_y - height;
        let best_x = self.skyline[best_idx].x;

        // Insert new skyline node for the placed rectangle.
        let new_seg = SkylineSeg {
            x: best_x,
            y: best_y,
            width,
        };

        self.skyline.insert(best_idx, new_seg);

        // Trim or remove segments covered by the new rectangle.
        let right_edge = best_x + width;
        let mut i = best_idx + 1;
        while i < self.skyline.len() {
            let seg_end = self.skyline[i].x + self.skyline[i].width;
            if self.skyline[i].x < right_edge {
                if seg_end <= right_edge {
                    // Fully covered — remove.
                    self.skyline.remove(i);
                } else {
                    // Partially covered — shrink from left.
                    let shrink = right_edge - self.skyline[i].x;
                    self.skyline[i].x += shrink;
                    self.skyline[i].width -= shrink;
                    break;
                }
            } else {
                break;
            }
        }

        self.merge_skyline();

        Some((best_x, top_y))
    }

    /// Check if a rect of `width` x `height` fits starting at skyline segment `idx`.
    /// Returns the Y coordinate of the top of the placed rect, or None if it doesn't fit.
    fn fit_at(&self, idx: usize, width: u32, height: u32) -> Option<u32> {
        let start_x = self.skyline[idx].x;
        let right_edge = start_x + width;
        if right_edge > self.atlas_width {
            return None;
        }

        let mut max_y = 0u32;
        let mut i = idx;

        while i < self.skyline.len() && self.skyline[i].x < right_edge {
            max_y = max_y.max(self.skyline[i].y);
            if max_y + height > self.atlas_height {
                return None;
            }
            i += 1;
        }

        Some(max_y)
    }

    /// Merge adjacent skyline segments at the same Y level.
    fn merge_skyline(&mut self) {
        let mut i = 0;
        while i + 1 < self.skyline.len() {
            if self.skyline[i].y == self.skyline[i + 1].y {
                self.skyline[i].width += self.skyline[i + 1].width;
                self.skyline.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }

    /// Rebuild both atlases after eviction.
    ///
    /// Evicts the oldest half of the LRU entries and re-packs remaining glyphs.
    fn rebuild(&mut self) {
        let evict_count = self.lru_keys.len() / 2;
        if evict_count == 0 && self.lru_keys.len() > 0 {
            self.cache.clear();
            self.stored_glyphs.clear();
            self.lru_keys.clear();
            self.skyline.clear();
            self.skyline.push(SkylineSeg { x: 0, y: 0, width: self.atlas_width });
            self.atlas_data.fill(0);
            self.color_skyline.clear();
            self.color_skyline.push(SkylineSeg { x: 0, y: 0, width: self.atlas_width });
            self.color_atlas_data.fill(0);
            self.dirty = true;
            self.color_dirty = true;
            return;
        }

        // PureTerm: use `drain(..evict_count)` to remove the oldest half
        // in O(n) (single allocation shift). Equivalent semantics to the
        // old `Vec::drain` path, but consistent with the new IndexMap.
        let evicted: Vec<_> = self.lru_keys.drain(..evict_count).collect();
        for (key, _) in &evicted {
            self.cache.remove(key);
            self.stored_glyphs.remove(key);
        }

        // Reset both atlas bitmaps and skylines.
        self.atlas_data.fill(0);
        self.skyline.clear();
        self.skyline.push(SkylineSeg { x: 0, y: 0, width: self.atlas_width });
        self.color_atlas_data.fill(0);
        self.color_skyline.clear();
        self.color_skyline.push(SkylineSeg { x: 0, y: 0, width: self.atlas_width });

        let remaining: Vec<(usize, char, RasterizedGlyph)> = self
            .stored_glyphs
            .iter()
            .map(|((fi, c), g)| (*fi, *c, Self::clone_rasterized(g)))
            .collect();

        self.cache.clear();
        self.stored_glyphs.clear();
        self.lru_keys.clear();

        for (font_index, c, glyph) in remaining {
            let is_color = matches!(&glyph.buffer, BitmapBuffer::Rgba(_));
            if is_color {
                let (w, h, rgba) = Self::extract_rgba(&glyph);
                if w == 0 || h == 0 { continue; }
                if let Some((x, y)) = self.allocate_color_rect(w, h) {
                    self.blit_color_glyph(x, y, w, h, &rgba);
                    let u = x as f32 / self.atlas_width as f32;
                    let v = y as f32 / self.atlas_height as f32;
                    let u2 = (x + w) as f32 / self.atlas_width as f32;
                    let v2 = (y + h) as f32 / self.atlas_height as f32;
                    let key = (font_index, c);
                    let cached = CachedGlyph {
                        u, v, u2, v2,
                        x_offset: glyph.left as f32,
                        y_offset: glyph.top as f32,
                        width: w as f32,
                        height: h as f32,
                        atlas_kind: AtlasKind::Color,
                    };
                    self.cache.insert(key, cached);
                    self.stored_glyphs.insert(key, Self::clone_rasterized(&glyph));
                    self.lru_keys.insert(key, ());
                }
            } else {
                let (w, h, gray) = Self::extract_grayscale(&glyph);
                if w == 0 || h == 0 { continue; }
                if let Some((x, y)) = self.allocate_rect(w, h) {
                    self.blit_glyph(x, y, w, h, &gray);
                    let u = x as f32 / self.atlas_width as f32;
                    let v = y as f32 / self.atlas_height as f32;
                    let u2 = (x + w) as f32 / self.atlas_width as f32;
                    let v2 = (y + h) as f32 / self.atlas_height as f32;
                    let key = (font_index, c);
                    let cached = CachedGlyph {
                        u, v, u2, v2,
                        x_offset: glyph.left as f32,
                        y_offset: glyph.top as f32,
                        width: w as f32,
                        height: h as f32,
                        atlas_kind: AtlasKind::Grayscale,
                    };
                    self.cache.insert(key, cached);
                    self.stored_glyphs.insert(key, Self::clone_rasterized(&glyph));
                    self.lru_keys.insert(key, ());
                }
            }
        }

        self.dirty = true;
        self.color_dirty = true;
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
        assert!(!atlas.is_color_dirty());
        assert_eq!(cached.width, 10.0);
        assert_eq!(cached.height, 14.0);
        assert_eq!(cached.atlas_kind, AtlasKind::Grayscale);
    }

    #[test]
    fn test_insert_color_glyph() {
        let mut atlas = WgpuGlyphAtlas::new();
        // Simulate a color emoji glyph (RGBA).
        let glyph = RasterizedGlyph {
            character: '🎉',
            top: 0,
            left: 0,
            width: 16,
            height: 16,
            buffer: BitmapBuffer::Rgba(vec![255u8; 16 * 16 * 4]),
            advance: (16, 16),
        };
        let cached = atlas.insert(0, '🎉', &glyph);
        assert!(atlas.is_color_dirty());
        assert_eq!(cached.width, 16.0);
        assert_eq!(cached.height, 16.0);
        assert_eq!(cached.atlas_kind, AtlasKind::Color);
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

    /// Helper: build a tiny 1x1 Rgb glyph.
    fn tiny_glyph(c: char) -> RasterizedGlyph {
        RasterizedGlyph {
            character: c,
            top: 0,
            left: 0,
            width: 1,
            height: 1,
            buffer: BitmapBuffer::Rgb(vec![0u8; 3]),
            advance: (1, 1),
        }
    }

    #[test]
    fn lru_get_promotes_to_back() {
        // PureTerm: IndexMap-backed LRU — `get` on a hit must move the
        // entry to the back so the next eviction spares it.
        let mut atlas = WgpuGlyphAtlas::new();
        atlas.insert(0, 'A', &tiny_glyph('A'));
        atlas.insert(0, 'B', &tiny_glyph('B'));
        atlas.insert(0, 'C', &tiny_glyph('C'));

        // Touch 'A' so it becomes most-recently-used.
        assert!(atlas.get(0, 'A').is_some());

        // The LRU order should now be: B, C, A.
        let order: Vec<_> = atlas.lru_keys.keys().copied().collect();
        assert_eq!(order, vec![(0, 'B'), (0, 'C'), (0, 'A')]);
    }

    #[test]
    fn lru_insert_existing_promotes_to_back() {
        let mut atlas = WgpuGlyphAtlas::new();
        atlas.insert(0, 'A', &tiny_glyph('A'));
        atlas.insert(0, 'B', &tiny_glyph('B'));
        atlas.insert(0, 'A', &tiny_glyph('A')); // re-insert 'A'

        // After re-insert, A should be at the back.
        let order: Vec<_> = atlas.lru_keys.keys().copied().collect();
        assert_eq!(order, vec![(0, 'B'), (0, 'A')]);
    }

    #[test]
    fn lru_drain_evicts_oldest() {
        // PureTerm: rebuild path uses `drain(..evict_count)` to remove the
        // oldest half in O(n) (single allocation shift).
        let mut atlas = WgpuGlyphAtlas::new();
        for c in "ABCDE".chars() {
            atlas.insert(0, c, &tiny_glyph(c));
        }
        // Order: A B C D E (A is oldest).
        let evicted: Vec<_> = atlas.lru_keys.drain(..2).collect();
        let evicted_keys: Vec<_> = evicted.into_iter().map(|(k, ())| k).collect();
        assert_eq!(evicted_keys, vec![(0, 'A'), (0, 'B')]);
        // Survivors in order: C D E.
        let survivors: Vec<_> = atlas.lru_keys.keys().copied().collect();
        assert_eq!(survivors, vec![(0, 'C'), (0, 'D'), (0, 'E')]);
    }

    /// PureTerm B3.1: `take_dirty_regions` returns per-glyph regions instead
    /// of forcing the render thread to re-upload the full atlas. A single
    /// new glyph (one of the 8192 cache slots) yields a single small rect.
    #[test]
    fn dirty_regions_track_per_glyph() {
        let mut atlas = WgpuGlyphAtlas::new();
        atlas.insert(0, 'A', &tiny_glyph('A'));
        atlas.insert(0, 'B', &tiny_glyph('B'));
        let dirty = atlas.take_dirty_regions().expect("atlas is dirty");
        assert_eq!(dirty.regions.len(), 2);
        // Each region is 1x1 (tiny_glyph).
        for r in &dirty.regions {
            assert_eq!(r.w, 1);
            assert_eq!(r.h, 1);
        }
        // After take, atlas is clean and a second take returns None.
        assert!(atlas.take_dirty_regions().is_none());
        // The Arc<data> shares the full atlas buffer (4MB).
        assert_eq!(dirty.data.len(), (ATLAS_SIZE * ATLAS_SIZE) as usize);
    }

    /// PureTerm B3.1: `take_color_dirty_regions` is the color-atlas
    /// counterpart.
    #[test]
    fn color_dirty_regions_track_per_glyph() {
        let mut atlas = WgpuGlyphAtlas::new();
        let color_glyph = RasterizedGlyph {
            character: '🎯',
            top: 0,
            left: 0,
            width: 2,
            height: 2,
            buffer: BitmapBuffer::Rgba(vec![0u8; 2 * 2 * 4]),
            advance: (2, 2),
        };
        atlas.insert(0, '🎯', &color_glyph);
        let dirty = atlas.take_color_dirty_regions().expect("color atlas is dirty");
        assert_eq!(dirty.regions.len(), 1);
        assert_eq!(dirty.regions[0].w, 2);
        assert_eq!(dirty.regions[0].h, 2);
        // Color atlas is 4 bytes per pixel.
        assert_eq!(dirty.data.len(), (ATLAS_SIZE * ATLAS_SIZE * 4) as usize);
    }
}
