//! T-core-graphics v3: Sixel/Kitty/iTerm2 内联图 quad pass 的状态与
//! per-image 描述符。
//!
//! - [`ImageQuadState`] 是 CPU 侧的图像 atlas(单张 2048×2048 RGBA8 纹理),
//!   暴露 insert/remove/get/take_dirty_data 等操作。
//! - [`ImageQuadDraw`] 是每帧由 `TerminalProgram::draw` 构建的「这张图要画
//!   在哪个网格矩形上」描述符,渲染线程据此发射 4 顶点 + 6 索引。

use pureterm_alacritty_runtime::renderer::image_atlas::{
    AtlasDirty as ImageAtlasDirty, ImageAtlas, ImagePlacement,
};

/// T-core-graphics v2: image quad pass state.
///
/// The image atlas is the CPU-side RGBA bitmap that backs Sixel / Kitty
/// / iTerm2 inline images. The render thread uploads dirty regions
/// to the GPU once per frame (see `take_dirty_data()`).
///
/// v2 limitation: the GPU quad-rendering pipeline (vertex + fragment
/// shader pair that samples the image atlas with bilinear filtering)
/// is wired in v3. For now `ImageQuadState` only owns the CPU side;
/// callers can read placements but not yet draw them.
pub struct ImageQuadState {
    pub atlas: ImageAtlas,
}

impl ImageQuadState {
    pub fn new() -> Self {
        Self {
            atlas: ImageAtlas::new(),
        }
    }

    /// Insert a decoded RGBA image into the atlas. Returns the
    /// `ImagePlacement` the GPU quad pass will sample from.
    pub fn insert(&mut self, id: u64, rgba: std::sync::Arc<Vec<u8>>, w: u32, h: u32) -> Option<ImagePlacement> {
        self.atlas.insert(id, rgba, w, h)
    }

    /// Remove an image (Kitty `d=d`, scrollback prune, etc.).
    pub fn remove(&mut self, id: u64) -> bool {
        self.atlas.remove(id)
    }

    /// Look up the GPU placement for an already-cached image.
    pub fn get(&mut self, id: u64) -> Option<ImagePlacement> {
        self.atlas.get(id)
    }

    /// Returns the dirty upload payload if any. The render thread is
    /// responsible for `queue.write_texture` to the image atlas.
    pub fn take_dirty_data(&mut self) -> Option<ImageAtlasDirty> {
        self.atlas.take_dirty_data()
    }

    pub fn is_dirty(&self) -> bool {
        self.atlas.is_dirty()
    }

    pub fn len(&self) -> usize {
        self.atlas.len()
    }
}

impl Default for ImageQuadState {
    fn default() -> Self {
        Self::new()
    }
}

/// T-core-graphics v3: per-image draw description consumed by the image
/// quad pass. The host (`TerminalProgram::draw`) builds a `Vec<ImageQuadDraw>`
/// each frame based on what's visible in the current viewport; the renderer
/// then emits 4 vertices + 6 indices per entry, mapping the GPU atlas UV
/// range to the screen-space rectangle `(col*cell_w, row*cell_h,
/// cols*cell_w, rows*cell_h)`.
///
/// `image_id` is the `GraphicsPlacement.id` from the original
/// `Event::GraphicsImage` — when the same id re-transmits (Kitty `a=T,i=…`),
/// the atlas entry is replaced and the new `ImagePlacement` UVs are used.
#[derive(Debug, Clone)]
pub struct ImageQuadDraw {
    pub image_id: u32,
    /// Grid-relative top-left column (logical px = col * cell_w).
    pub col: u16,
    /// Grid-relative top-left row (logical px = row * cell_h).
    pub row: u16,
    /// Image width in cells.
    pub cols: u16,
    /// Image height in cells.
    pub rows: u16,
    /// Z-order — newer images overlay older ones. Monotonically increasing
    /// at insertion time on the CPU side (see `ImageStore::insert`).
    pub z: u32,
}
