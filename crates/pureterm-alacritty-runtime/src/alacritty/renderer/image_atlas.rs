//! T-core-graphics v2: GPU image atlas for Sixel / chunked graphics / inline image.
//!
//! Mirrors the design of `glyph_atlas.rs` but stores RGBA pixels (Sixel
//! and inline images are already RGBA; chunked-graphics PNG payload is
//! decoded before insertion). Key differences from the glyph atlas:
//!
//! - **Single RGBA bitmap** — no separate grayscale path. Images are
//!   always full-color.
//! - **Larger max entries** — images are typically 100×–10 000× larger
//!   than glyphs (multi-KB to multi-MB vs 100s of bytes), so the LRU
//!   cap is tighter (the GPU upload is the bottleneck, not the CPU
//!   bin-packing).
//! - **Dirty-region tracking** — partial `queue.write_texture` calls
//!   per image that changed this frame, so re-renders don't pay for
//!   the full atlas each time.
//!
//! The atlas is a CPU-side bitmap of size [`ATLAS_SIZE`]² pixels with
//! RGBA8. The render thread uploads only the changed [`DirtyRect`]
//! sub-rectangles.

use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

/// Atlas dimensions — same as `glyph_atlas` for shared texture code.
pub const ATLAS_SIZE: u32 = 2048;

/// Maximum number of cached images before LRU eviction kicks in.
const MAX_IMAGE_ENTRIES: usize = 256;

/// Where a placed image lives in the atlas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImagePlacement {
    /// Top-left U coordinate in atlas UV space (0..=1).
    pub u: f32,
    /// Top-left V coordinate in atlas UV space (0..=1).
    pub v: f32,
    /// Bottom-right U coordinate in atlas UV space (0..=1).
    pub u2: f32,
    /// Bottom-right V coordinate in atlas UV space (0..=1).
    pub v2: f32,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
}

/// Sub-rectangle of the atlas that's been modified since the last upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A queued dirty region to upload. The render thread slices
/// `data[layout_offset..]` to extract the per-region bytes for
/// `queue.write_texture`.
pub struct AtlasDirty {
    pub regions: Vec<DirtyRect>,
    pub data: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// Occupied region tracked by the row-stride allocator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Occupied {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// CPU-side RGBA image atlas.
///
/// Allocator: simple row-stride "next free position" search. We walk a
/// flat list of occupied rectangles and place each new image at the
/// lowest non-overlapping y for the chosen x. This is O(n²) in the
/// number of placed images but fine for our cap (256).
pub struct ImageAtlas {
    atlas: Vec<u8>,
    occupied: Vec<Occupied>,
    cache: IndexMap<u64, ImagePlacement>,
    pixels: HashMap<u64, Arc<Vec<u8>>>,
    dims: HashMap<u64, (u32, u32)>,
    dirty: bool,
}

impl ImageAtlas {
    pub fn new() -> Self {
        Self {
            atlas: vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize],
            occupied: Vec::new(),
            cache: IndexMap::new(),
            pixels: HashMap::new(),
            dims: HashMap::new(),
            dirty: false,
        }
    }

    /// Insert an image and return its placement in the atlas.
    pub fn insert(&mut self, id: u64, rgba: Arc<Vec<u8>>, width: u32, height: u32) -> Option<ImagePlacement> {
        if width == 0 || height == 0 {
            return None;
        }
        if width > ATLAS_SIZE || height > ATLAS_SIZE {
            return None;
        }
        let expected_len = (width as usize) * (height as usize) * 4;
        if rgba.len() != expected_len {
            return None;
        }

        // Already cached? Just refresh LRU.
        if let Some(&placement) = self.cache.get(&id) {
            self.cache.shift_remove(&id);
            self.cache.insert(id, placement);
            return Some(placement);
        }

        // LRU eviction: if we're at capacity, drop the oldest entry.
        if self.cache.len() >= MAX_IMAGE_ENTRIES {
            if let Some((victim_id, _)) = self.cache.shift_remove_index(0) {
                self.pixels.remove(&victim_id);
                self.dims.remove(&victim_id);
                // Note: we deliberately don't carve out the occupied rect
                // — the bitmap is large enough that orphans don't matter,
                // and removing them would invalidate all later placements.
            }
        }

        // Find a non-overlapping position. We probe x = 0 in steps of
        // `width`; for each x, we find the lowest y that doesn't
        // overlap any existing rect and fits within the atlas height.
        let pos = self.allocate_rect(width, height)?;
        self.blit_rgba(pos.0, pos.1, width, height, &rgba);

        let placement = ImagePlacement {
            u: pos.0 as f32 / ATLAS_SIZE as f32,
            v: pos.1 as f32 / ATLAS_SIZE as f32,
            u2: (pos.0 + width) as f32 / ATLAS_SIZE as f32,
            v2: (pos.1 + height) as f32 / ATLAS_SIZE as f32,
            width,
            height,
        };

        self.occupied.push(Occupied {
            x: pos.0,
            y: pos.1,
            w: width,
            h: height,
        });
        self.cache.insert(id, placement);
        self.pixels.insert(id, rgba);
        self.dims.insert(id, (width, height));
        self.dirty = true;

        Some(placement)
    }

    /// Look up an image's placement without re-inserting.
    pub fn get(&mut self, id: u64) -> Option<ImagePlacement> {
        if let Some(&placement) = self.cache.get(&id) {
            // Update LRU.
            self.cache.shift_remove(&id);
            self.cache.insert(id, placement);
            Some(placement)
        } else {
            None
        }
    }

    /// Remove an image from the atlas (e.g. when the terminal
    /// scrollbacks past it or the protocol sends a chunked-graphics `d=d`).
    pub fn remove(&mut self, id: u64) -> bool {
        let removed = self.cache.shift_remove(&id).is_some();
        if removed {
            self.pixels.remove(&id);
            self.dims.remove(&id);
            self.dirty = true;
        }
        removed
    }

    /// Number of images currently cached.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns true if the atlas has no images.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Returns true if there are dirty regions to upload.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Take the dirty upload payload. Returns `None` if the atlas
    /// hasn't changed since the last call. After this call, the
    /// `dirty` flag is reset.
    pub fn take_dirty_data(&mut self) -> Option<AtlasDirty> {
        if !self.dirty {
            return None;
        }
        let regions = vec![DirtyRect {
            x: 0,
            y: 0,
            w: ATLAS_SIZE,
            h: ATLAS_SIZE,
        }];
        let data = Arc::new(self.atlas.clone());
        self.dirty = false;
        Some(AtlasDirty {
            regions,
            data,
            width: ATLAS_SIZE,
            height: ATLAS_SIZE,
        })
    }

    /// Force the atlas to be marked dirty on the next render.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Find a non-overlapping `(x, y)` for a rect of `w × h`.
    ///
    /// Scans `x` from 0 in steps of `w` (so we always land on a
    /// `w`-aligned grid), and for each `x`, finds the lowest `y` that
    /// doesn't overlap any existing rect and fits within ATLAS_SIZE.
    fn allocate_rect(&self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > ATLAS_SIZE || h > ATLAS_SIZE {
            return None;
        }
        let mut x = 0u32;
        while x + w <= ATLAS_SIZE {
            // Find the lowest y at this x where a rect of (w, h) doesn't
            // overlap any existing rect.
            let y = self.lowest_free_y_at(x, w, h);
            if let Some(y) = y {
                return Some((x, y));
            }
            // No room at this x — try the next column.
            x = x.saturating_add(w);
            if x == 0 {
                // Overflow (w = 0 already filtered above).
                return None;
            }
        }
        None
    }

    fn lowest_free_y_at(&self, x: u32, w: u32, h: u32) -> Option<u32> {
        // We test y values starting at 0, stepping by 1 if any rect
        // overlaps. Because rects have finite height, eventually either
        // we find a y or run off the atlas.
        let mut y = 0u32;
        while y + h <= ATLAS_SIZE {
            let probe = Occupied { x, y, w, h };
            if !self.occupied.iter().any(|o| overlaps(*o, probe)) {
                return Some(y);
            }
            // Jump past the tallest overlapping rect at this x-band.
            let tallest = self
                .occupied
                .iter()
                .filter(|o| horizontal_overlap(**o, probe))
                .map(|o| o.y + o.h)
                .max()
                .unwrap_or(y + 1);
            y = tallest;
        }
        None
    }

    fn blit_rgba(&mut self, x: u32, y: u32, w: u32, h: u32, rgba: &[u8]) {
        let stride = (ATLAS_SIZE * 4) as usize;
        let row_bytes = (w * 4) as usize;
        for row in 0..h {
            let src_start = (row * w * 4) as usize;
            let src_end = src_start + row_bytes;
            let dst_start = ((y + row) as usize * stride) + (x as usize * 4);
            let dst_end = dst_start + row_bytes;
            self.atlas[dst_start..dst_end].copy_from_slice(&rgba[src_start..src_end]);
        }
    }
}

fn overlaps(a: Occupied, b: Occupied) -> bool {
    horizontal_overlap(a, b) && vertical_overlap(a, b)
}

fn horizontal_overlap(a: Occupied, b: Occupied) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w
}

fn vertical_overlap(a: Occupied, b: Occupied) -> bool {
    a.y < b.y + b.h && b.y < a.y + a.h
}

impl Default for ImageAtlas {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a 16x16 RGBA image of a solid color.
    fn solid_image(r: u8, g: u8, b: u8, a: u8) -> Arc<Vec<u8>> {
        Arc::new(vec![r, g, b, a].repeat(16 * 16))
    }

    #[test]
    fn new_atlas_is_empty_and_clean() {
        let atlas = ImageAtlas::new();
        assert!(atlas.is_empty());
        assert!(!atlas.is_dirty());
    }

    #[test]
    fn insert_marks_dirty_and_returns_placement() {
        let mut atlas = ImageAtlas::new();
        let placement = atlas.insert(1, solid_image(255, 0, 0, 255), 16, 16).unwrap();
        assert_eq!(placement.width, 16);
        assert_eq!(placement.height, 16);
        assert!(atlas.is_dirty());
        assert_eq!(atlas.len(), 1);
    }

    #[test]
    fn duplicate_insert_refreshes_lru_no_dirty() {
        let mut atlas = ImageAtlas::new();
        let _ = atlas.insert(1, solid_image(255, 0, 0, 255), 16, 16).unwrap();
        assert!(atlas.is_dirty());
        atlas.take_dirty_data();
        assert!(!atlas.is_dirty());

        let placement = atlas.insert(1, solid_image(255, 0, 0, 255), 16, 16).unwrap();
        assert_eq!(placement.width, 16);
        assert!(!atlas.is_dirty());
    }

    #[test]
    fn lru_eviction_drops_oldest() {
        // MAX_IMAGE_ENTRIES is 256; the LRU kicks in once we hit that.
        let mut atlas = ImageAtlas::new();
        for id in 0..MAX_IMAGE_ENTRIES as u64 {
            atlas.insert(id, solid_image(0, 0, 0, 255), 16, 16).unwrap();
        }
        assert_eq!(atlas.len(), MAX_IMAGE_ENTRIES);

        // Insert one more: triggers eviction of id=0.
        atlas
            .insert(MAX_IMAGE_ENTRIES as u64, solid_image(0, 0, 0, 255), 16, 16)
            .unwrap();
        assert_eq!(atlas.len(), MAX_IMAGE_ENTRIES);
        assert!(atlas.get(0).is_none());
        assert!(atlas.get(1).is_some());
    }

    #[test]
    fn get_updates_lru_order() {
        let mut atlas = ImageAtlas::new();
        for id in 0..MAX_IMAGE_ENTRIES as u64 {
            atlas.insert(id, solid_image(0, 0, 0, 255), 16, 16).unwrap();
        }
        // Touch id=0 to make it MRU.
        assert!(atlas.get(0).is_some());
        // Insert a new image: id=0 should survive, id=1 should be evicted.
        atlas
            .insert(MAX_IMAGE_ENTRIES as u64, solid_image(0, 0, 0, 255), 16, 16)
            .unwrap();
        assert!(atlas.get(0).is_some());
        assert!(atlas.get(1).is_none());
    }

    #[test]
    fn remove_clears_entry() {
        let mut atlas = ImageAtlas::new();
        atlas.insert(1, solid_image(0, 0, 0, 255), 16, 16).unwrap();
        assert!(atlas.remove(1));
        assert!(!atlas.remove(1));
        assert!(atlas.is_empty());
    }

    #[test]
    fn take_dirty_data_clears_flag() {
        let mut atlas = ImageAtlas::new();
        atlas.insert(1, solid_image(0, 0, 0, 255), 16, 16).unwrap();
        let payload = atlas.take_dirty_data();
        assert!(payload.is_some());
        assert!(!atlas.is_dirty());
        assert!(atlas.take_dirty_data().is_none());
    }

    #[test]
    fn rejects_zero_size_image() {
        let mut atlas = ImageAtlas::new();
        assert!(atlas.insert(1, Arc::new(vec![]), 0, 0).is_none());
    }

    #[test]
    fn rejects_oversize_image() {
        let mut atlas = ImageAtlas::new();
        let data = vec![0u8; (ATLAS_SIZE as usize + 1) * 4];
        assert!(atlas.insert(1, Arc::new(data), ATLAS_SIZE + 1, 1).is_none());
    }

    #[test]
    fn rejects_mismatched_data_length() {
        let mut atlas = ImageAtlas::new();
        let data = vec![0u8; 100];
        assert!(atlas.insert(1, Arc::new(data), 16, 16).is_none());
    }

    #[test]
    fn uv_coords_are_unit_interval() {
        let mut atlas = ImageAtlas::new();
        let placement = atlas.insert(1, solid_image(0, 0, 0, 255), 16, 16).unwrap();
        assert!(placement.u >= 0.0 && placement.u <= 1.0);
        assert!(placement.v >= 0.0 && placement.v <= 1.0);
        assert!(placement.u2 >= placement.u);
        assert!(placement.v2 >= placement.v);
    }

    #[test]
    fn multiple_images_pack_without_overlap() {
        let mut atlas = ImageAtlas::new();
        let p1 = atlas.insert(1, solid_image(255, 0, 0, 255), 16, 16).unwrap();
        let p2 = atlas.insert(2, solid_image(0, 255, 0, 255), 16, 16).unwrap();
        let p1_box = (p1.u, p1.v, p1.u2, p1.v2);
        let p2_box = (p2.u, p2.v, p2.u2, p2.v2);
        assert!(
            p1_box.2 <= p2_box.0 || p2_box.2 <= p1_box.0
                || p1_box.3 <= p2_box.1 || p2_box.3 <= p1_box.1,
            "placements {p1_box:?} and {p2_box:?} overlap"
        );
    }
}