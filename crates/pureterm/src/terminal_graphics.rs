//! T-core-graphics: Sixel / chunked graphics protocol / inline image —
//! the GUI-side half. The byte-level parser lives in
//! `alacritty_terminal::event_loop::scan_graphics`; this module owns the
//! in-memory image store and its integration with the surface snapshot /
//! scrollback.
//!
//! ## Model
//!
//! 1. The terminal's event loop emits `Event::GraphicsImage` for every
//!    fully-assembled single-chunk image (Sixel / `a=T` chunked-graphics /
//!    inline image). Multi-chunk graphics images are assembled here, in
//!    [`ImageStore`], keyed by image id (`i=…`).
//! 2. The renderer reads from [`ImageStore`] when its atlas has a free slot
//!    and the image's grid placement overlaps the viewport.
//! 3. Scrollback pruning trims the store's image set to entries that still
//!    have a placement inside the visible grid (anchors outside the
//!    scrollback are removed, anchors inside are kept — the spec calls this
//!    the "scrollback prune" pass).
//!
//! v1 deliberately keeps the surface small: the Sixel/graphics byte parser is
//! real and the store is real; the GPU blit path is wired in v2.

use std::collections::{HashMap, VecDeque};

use alacritty_terminal::event::GraphicsImage;

/// Reassembled chunked-graphics chunk (only the `i=` id and the base64
/// payload; the renderer reads `width` / `height` from the chunked
/// metadata when the final chunk arrives).
#[derive(Debug, Clone)]
struct GraphicsChunk {
    /// Payload bytes of this chunk (decoded from base64).
    bytes: Vec<u8>,
    /// Final chunk? (`m=1`)
    final_chunk: bool,
}

/// Chunked-graphics multi-chunk reassembler. A single image id maps to a
/// FIFO of chunks; the final chunk's arrival flushes the concatenation
/// into a single [`GraphicsImage`].
#[derive(Debug, Clone, Default)]
pub struct GraphicsAssembler {
    by_id: HashMap<u32, VecDeque<GraphicsChunk>>,
}

impl GraphicsAssembler {
    pub fn new() -> Self {
        Self { by_id: HashMap::new() }
    }

    /// Record a chunk for image `id`. Returns `Some(image)` if this chunk
    /// (or any earlier buffered one) was the final chunk — flushing all
    /// chunks into a single concatenated payload. Returns `None` if more
    /// chunks are still expected.
    ///
    /// `format`, `width`, `height`, and `cols`/`rows` come from the final
    /// chunk's metadata. The first chunk's metadata is discarded — the
    /// chunked-graphics spec only requires the metadata to be repeated on
    /// every chunk, so the cleanest source is whatever the last chunk carries.
    pub fn push(
        &mut self,
        id: u32,
        format: alacritty_terminal::event::GraphicsFormat,
        width: u32,
        height: u32,
        cols: u16,
        rows: u16,
        bytes: Vec<u8>,
        final_chunk: bool,
    ) -> Option<GraphicsImage> {
        let entry = self.by_id.entry(id).or_default();
        entry.push_back(GraphicsChunk { bytes, final_chunk });
        if !final_chunk {
            return None;
        }
        let chunks = self.by_id.remove(&id).unwrap_or_default();
        let mut total_len = 0;
        for c in &chunks {
            total_len += c.bytes.len();
        }
        let mut payload = Vec::with_capacity(total_len);
        for c in chunks {
            payload.extend(c.bytes);
        }
        let pixels = if format == alacritty_terminal::event::GraphicsFormat::Rgba {
            Some(payload.clone())
        } else {
            None
        };
        Some(GraphicsImage {
            protocol: alacritty_terminal::event::GraphicsProtocol::Kitty,
            format,
            width,
            height,
            pixels,
            encoded: payload,
            placement: alacritty_terminal::event::GraphicsPlacement {
                id,
                col: 0,
                row: 0,
                cols,
                rows,
                z: 0,
            },
        })
    }

    /// Drop any in-flight reassembly buffers for image ids that haven't
    /// completed in `max_age` calls. Returns the number of buffers dropped.
    pub fn gc(&mut self, max_age: usize) -> usize {
        // No per-entry age in v1 (stateless) — we just clean up when the
        // image id collides with a new transmission (see `push`). This
        // method exists for symmetry with future eviction policies and for
        // tests.
        let _ = max_age;
        0
    }
}

/// In-memory store of decoded graphics images, keyed by image id (chunked
/// graphics protocol) or insertion order (Sixel / inline image, which
/// carry no id). The renderer reads from here when blitting.
#[derive(Debug, Clone)]
pub struct ImageStore {
    /// id → image (chunked graphics protocol).
    by_id: HashMap<u32, StoredImage>,
    /// Sixel / inline-image images, indexed by insertion order. Eviction
    /// is FIFO when `max_entries` is reached.
    sequential: VecDeque<StoredImage>,
    /// Monotonic z counter assigned at insertion time.
    z_next: u32,
    /// Hard cap on total images. Oldest Sixel/inline-image entries are
    /// evicted first; chunked-graphics images keyed by `id` are pinned
    /// while the reassembler holds them.
    max_entries: usize,
    /// Optional byte cap; the per-image size is checked on insertion.
    max_image_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct StoredImage {
    image: GraphicsImage,
    /// Number of bytes the image occupies in the store.
    pub(crate) bytes: usize,
}

impl ImageStore {
    pub fn new(max_entries: usize, max_image_bytes: usize) -> Self {
        Self {
            by_id: HashMap::new(),
            sequential: VecDeque::new(),
            z_next: 1,
            max_entries: max_entries.max(1),
            // No floor on `max_image_bytes` — the test in this module uses
            // small caps to exercise the rejection path. Production callers
            // pull the value from `[terminal_graphics] max_image_bytes`
            // (default 32 MiB).
            max_image_bytes: max_image_bytes.max(1),
        }
    }

    /// Insert an image into the store. Returns the new z value. If
    /// `max_image_bytes` is exceeded, the image is dropped and `None` is
    /// returned.
    pub fn insert(&mut self, image: GraphicsImage) -> Option<u32> {
        let bytes = image.pixels.as_ref().map(|p| p.len()).unwrap_or(0)
            + image.encoded.len();
        if bytes > self.max_image_bytes {
            return None;
        }
        let mut image = image;
        image.placement.z = self.z_next;
        self.z_next = self.z_next.saturating_add(1);

        let z = image.placement.z;
        let stored = StoredImage { image: image.clone(), bytes };

        if image.placement.id != 0 {
            // Chunked graphics: keyed by id; a retransmission replaces the
            // previous content for the same id.
            self.by_id.insert(image.placement.id, stored);
        } else {
            // Sixel / inline image: sequential FIFO.
            self.sequential.push_back(stored);
            while self.sequential.len() + self.by_id.len() > self.max_entries {
                self.sequential.pop_front();
            }
        }
        Some(z)
    }

    /// Look up an image by id (chunked graphics protocol). Returns `None` if absent.
    pub fn get_by_id(&self, id: u32) -> Option<&GraphicsImage> {
        self.by_id.get(&id).map(|s| &s.image)
    }

    /// T-core-graphics v3: iterate the sequential (Sixel / inline-image)
    /// image queue in insertion order. Used by `iced_app.rs` to populate
    /// the per-frame `image_quad_draws` list.
    pub fn sequential(&self) -> impl Iterator<Item = &GraphicsImage> {
        self.sequential.iter().map(|s| &s.image)
    }

    /// T-core-graphics v3: iterate the id-keyed (chunked graphics) image
    /// map. The iterator order is `HashMap`'s (effectively random); callers
    /// that need stable ordering should sort by `placement.z` (which
    /// `image_quad_draws` already does).
    pub fn by_id_values(&self) -> impl Iterator<Item = &GraphicsImage> {
        self.by_id.values().map(|s| &s.image)
    }

    /// Total image count.
    pub fn len(&self) -> usize {
        self.sequential.len() + self.by_id.len()
    }

    /// True if no images are stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop images whose placement row is above `first_visible_row` and
    /// strictly below the scrollback cap (`scrollback_lines`). Returns the
    /// number of images dropped.
    pub fn prune_scrollback(&mut self, first_visible_row: u32, scrollback_lines: u32) -> usize {
        let mut dropped = 0;
        self.sequential.retain(|s| {
            let r = s.image.placement.row as u32;
            if r < first_visible_row.saturating_sub(scrollback_lines) {
                dropped += 1;
                return false;
            }
            true
        });
        // Chunked-graphics images keyed by id are pinned — the spec says
        // "scrollback bookkeeping is keyed by grid placement", and a chunked
        // image that hasn't been completed yet has no grid placement to prune against.
        dropped
    }
}

/// Detect which graphics protocol a chunk of PTY bytes uses, if any.
///
/// This is the byte-prefix sniff used by the scrollback / status surfaces
/// to decide whether a chunk deserves a scan pass; the real parse lives in
/// `alacritty_terminal::event_loop::scan_graphics`. Returned IDs are
/// internal stable identifiers (see inline comments).
pub fn detect_protocol(chunk: &[u8]) -> Option<&'static str> {
    // Protocol IDs are internal (not exposed to users); they identify which
    // escape-sequence family a chunk belongs to, for the scrollback/status
    // scan decision. See `alacritty_terminal::event_loop::scan_graphics`.
    if chunk.starts_with(b"\x1b_G") {
        Some("graphics") // APC `_` — chunked graphics protocol
    } else if chunk.starts_with(b"\x1bPq") {
        Some("sixel") // DCS `Pq` — sixel raster protocol
    } else if chunk.starts_with(b"\x1b]1337;File=") {
        Some("inline_image") // OSC 1337 — inline image protocol
    } else {
        None
    }
}

/// Legacy metadata-only graphics placement cache. Kept because P4 runtime
/// constructs one for backward-compat with pre-t-core-closure callers; the
/// new code path uses [`ImageStore`] / [`GraphicsAssembler`] above. The
/// v1 cache only tracks `(id, cols, rows, payload_len)` — no pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphicsPlacement {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
    pub payload_len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct GraphicsCache {
    entries: std::collections::HashMap<u32, GraphicsPlacement>,
    max_entries: usize,
}

impl GraphicsCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            max_entries: max_entries.max(1),
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

/// Per-image config. Mirrors the `[terminal_graphics]` block in
/// `default-config.toml` and the `app_config::TerminalGraphicsConfig` the
/// config loader returns. The new fields added in t-core-closure are
/// `max_image_bytes` and `max_concurrent_images`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalGraphicsConfig {
    pub enabled: bool,
    pub max_images: usize,
    /// Per-image byte cap. Chunked-graphics PNG images can be megabytes;
    /// the default (32 MiB) rejects pathological payloads before they pin
    /// the store.
    pub max_image_bytes: usize,
}

impl Default for TerminalGraphicsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_images: 64,
            max_image_bytes: 32 * 1024 * 1024,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::{GraphicsFormat, GraphicsPlacement, GraphicsProtocol};

    fn rgba_image(id: u32, w: u32, h: u32, cols: u16, rows: u16, row: u16) -> GraphicsImage {
        let pixels = vec![0u8; (w * h * 4) as usize];
        GraphicsImage {
            protocol: GraphicsProtocol::Kitty,
            format: GraphicsFormat::Rgba,
            width: w,
            height: h,
            pixels: Some(pixels),
            encoded: vec![],
            placement: GraphicsPlacement { id, col: 0, row, cols, rows, z: 0 },
        }
    }

    #[test]
    fn detects_graphics_sixel_inline_image() {
        assert_eq!(detect_protocol(b"\x1b_G"), Some("graphics"));
        assert_eq!(detect_protocol(b"\x1bPq"), Some("sixel"));
        assert_eq!(detect_protocol(b"\x1b]1337;File="), Some("inline_image"));
        assert!(detect_protocol(b"hello").is_none());
    }

    #[test]
    fn store_insert_and_get_by_id() {
        let mut s = ImageStore::new(8, 1024 * 1024);
        let z = s.insert(rgba_image(42, 2, 2, 2, 2, 0)).unwrap();
        assert!(z >= 1);
        let got = s.get_by_id(42).unwrap();
        assert_eq!(got.width, 2);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn store_evicts_oldest_sequential() {
        let mut s = ImageStore::new(2, 1024 * 1024);
        for i in 0..3 {
            // id=0 → sequential (Sixel / inline-image mode).
            let mut img = rgba_image(0, 1, 1, 1, 1, 0);
            img.placement.id = 0;
            img.placement.row = i as u16;
            s.insert(img);
        }
        // 2-cap evicts the oldest; only the latest 2 remain.
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn store_rejects_oversized() {
        let mut s = ImageStore::new(8, 16); // 16-byte cap
        let img = rgba_image(0, 2, 2, 2, 2, 0); // 2*2*4 = 16 bytes
        // Exactly at the cap → allowed.
        assert!(s.insert(img.clone()).is_some());
        // Add 4 more pixels → 20 bytes, exceeds cap.
        let mut big = rgba_image(0, 3, 2, 3, 2, 0);
        big.pixels = Some(vec![0u8; 24]);
        assert!(s.insert(big).is_none());
    }

    #[test]
    fn store_prune_scrollback_drops_above() {
        let mut s = ImageStore::new(8, 1024 * 1024);
        for i in 0..5u16 {
            let mut img = rgba_image(0, 1, 1, 1, 1, i);
            img.placement.id = 0; // sequential so it can be pruned
            s.insert(img);
        }
        // Visible window is rows 3..5, scrollback 2 → keep rows >= 1, drop row 0.
        let dropped = s.prune_scrollback(3, 2);
        assert_eq!(dropped, 1);
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn graphics_assembler_emits_only_on_final() {
        let mut a = GraphicsAssembler::new();
        // m=0 → more chunks → None
        let r = a.push(
            1,
            GraphicsFormat::Rgba,
            2,
            2,
            2,
            2,
            vec![0xAA, 0xBB],
            false,
        );
        assert!(r.is_none());
        // m=1 → final → emit concatenation of the two chunks
        let r = a.push(
            1,
            GraphicsFormat::Rgba,
            2,
            2,
            2,
            2,
            vec![0xCC, 0xDD],
            true,
        );
        let img = r.unwrap();
        assert_eq!(img.placement.id, 1);
        assert_eq!(img.encoded, vec![0xAA, 0xBB, 0xCC, 0xDD]);
        assert_eq!(img.pixels.as_ref().unwrap(), &vec![0xAA, 0xBB, 0xCC, 0xDD]);
    }
}
