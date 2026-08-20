//! T-core-graphics v3: Sixel/chunked-graphics/inline-image 内联图 quad pass
//! 的状态与 per-image 描述符。
//!
//! - [`ImageQuadState`] 是 CPU 侧的图像 atlas(单张 2048×2048 RGBA8 纹理),
//!   暴露 insert/remove/get/take_dirty_data 等操作。
//! - [`ImageQuadDraw`] 是每帧由 `TerminalProgram::draw` 构建的「这张图要画
//!   在哪个网格矩形上」描述符,渲染线程据此发射 4 顶点 + 6 索引。

use pureterm_alacritty_runtime::renderer::image_atlas::{
    AtlasDirty as ImageAtlasDirty, ImageAtlas, ImagePlacement,
};

/// T-core-graphics v2:图像四边形(image quad)渲染阶段状态。
///
/// 图像图集是支撑 Sixel / 分块图形 / 内联图像的 CPU 侧 RGBA 位图。
/// 渲染线程每帧上传一次
/// 脏区域到 GPU(见 `take_dirty_data()`)。
///
/// v2 限制:GPU 四边形渲染管线(顶点 + 片元
/// 着色器对,以双线性过滤采样图像图集)
/// 在 v3 中接入。当前 `ImageQuadState` 只持有 CPU 侧;
/// 调用方可读取位置,但尚不能绘制。
pub struct ImageQuadState {
    pub atlas: ImageAtlas,
}

impl ImageQuadState {
    pub fn new() -> Self {
        Self {
            atlas: ImageAtlas::new(),
        }
    }

    /// 把解码后的 RGBA 图像插入图集。返回
    /// GPU 四边形渲染阶段将采样的 `ImagePlacement`。
    pub fn insert(&mut self, id: u64, rgba: std::sync::Arc<Vec<u8>>, w: u32, h: u32) -> Option<ImagePlacement> {
        self.atlas.insert(id, rgba, w, h)
    }

    /// 移除一张图像(分块图形 `d=d`、回滚裁剪等)。
    pub fn remove(&mut self, id: u64) -> bool {
        self.atlas.remove(id)
    }

    /// 查找已缓存图像的 GPU 位置。
    pub fn get(&mut self, id: u64) -> Option<ImagePlacement> {
        self.atlas.get(id)
    }

    /// 返回有待上传的脏负载(若有)。渲染线程负责
    /// 对图像图集执行 `queue.write_texture`。
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

/// T-core-graphics v3:图像四边形渲染阶段消费的逐图绘制描述。
/// 宿主(`TerminalProgram::draw`)根据当前视口可见内容
/// 每帧构建 `Vec<ImageQuadDraw>`;渲染器随后
/// 为每个条目发出 4 个顶点 + 6 个索引,把 GPU 图集 UV 范围
/// 映射到屏幕空间矩形 `(col*cell_w, row*cell_h, cols*cell_w, rows*cell_h)`。
///
///
/// `image_id` 来自原始事件的 `GraphicsPlacement.id`:
/// `Event::GraphicsImage` —— 同一 id 重传时(分块图形
/// `a=T,i=…`),图集条目被替换,并采用新的 `ImagePlacement` UV。
#[derive(Debug, Clone)]
pub struct ImageQuadDraw {
    pub image_id: u32,
    /// 相对网格的左上角列(逻辑像素 = col * cell_w)。
    pub col: u16,
    /// 相对网格的左上角行(逻辑像素 = row * cell_h)。
    pub row: u16,
    /// 图像宽度(以单元格计)。
    pub cols: u16,
    /// 图像高度(以单元格计)。
    pub rows: u16,
    /// Z 序 —— 较新的图像覆盖较旧的。在 CPU 侧插入时
    /// 单调递增(见 `ImageStore::insert`)。
    pub z: u32,
}
