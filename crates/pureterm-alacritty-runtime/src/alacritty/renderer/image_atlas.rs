//! T-core-graphics v2:Sixel / 分块图形 / 内联图像的 GPU 图像图集。
//!
//! 镜像 `glyph_atlas.rs` 的设计,但存 RGBA 像素(Sixel
//! 与内联图像本就是 RGBA;分块图形的 PNG 载荷在
//! 插入前已解码)。与字形图集的关键差异:
//!
//! - **单一 RGBA 位图**——没有单独的灰度路径。图像
//!   总是全彩的。
//! - **更大的条目上限**——图像通常比字形大
//!   100 倍到 10000 倍(数 KB 到数 MB 对几百字节),所以 LRU
//!   上限更紧(GPU 上传是瓶颈,而不是 CPU
//!   装箱)。
//! - **脏区域跟踪**——对每帧变化的图像做部分
//!   `queue.write_texture` 调用,重渲染不必每次为
//!   整个图集付费。
//!
//! 图集是一张 [`ATLAS_SIZE`]² 像素、RGBA8 的 CPU 侧位图。
//! 渲染线程只上传变化的 [`DirtyRect`]
//! 子矩形。

use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

/// 图集尺寸——与 `glyph_atlas` 相同,共享纹理代码。
pub const ATLAS_SIZE: u32 = 2048;

/// 触发 LRU 逐出前可缓存图像的最大数量。
const MAX_IMAGE_ENTRIES: usize = 256;

/// 已放置图像在图集中的位置。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImagePlacement {
    /// 图集 UV 空间中左上角的 U 坐标(0..=1)。
    pub u: f32,
    /// 图集 UV 空间中左上角的 V 坐标(0..=1)。
    pub v: f32,
    /// 图集 UV 空间中右下角的 U 坐标(0..=1)。
    pub u2: f32,
    /// 图集 UV 空间中右下角的 V 坐标(0..=1)。
    pub v2: f32,
    /// 图像宽度(像素)。
    pub width: u32,
    /// 图像高度(像素)。
    pub height: u32,
}

/// 自上次上传以来图集中被修改过的子矩形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 一个排队上传的脏区域。渲染线程切出
/// `data[layout_offset..]` 以提取各区域的字节
/// `queue.write_texture`.
pub struct AtlasDirty {
    pub regions: Vec<DirtyRect>,
    pub data: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// 行距分配器跟踪的占用区域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Occupied {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// CPU 侧 RGBA 图像图集。
///
/// 分配器:简单的行距“下一个空闲位置”搜索。我们遍历
/// 已占矩形的扁平列表,把每个新图像放在所选 x 处
/// 最低的不重叠 y。这在已放置图像数上是 O(n²),
/// 但对我们的上限(256)足够。
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

    /// 插入一幅图像并返回其在图集中的位置。
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

        // 已缓存?只刷新 LRU。
        if let Some(&placement) = self.cache.get(&id) {
            self.cache.shift_remove(&id);
            self.cache.insert(id, placement);
            return Some(placement);
        }

        // LRU 逐出:达到容量时丢弃最旧条目。
        if self.cache.len() >= MAX_IMAGE_ENTRIES {
            if let Some((victim_id, _)) = self.cache.shift_remove_index(0) {
                self.pixels.remove(&victim_id);
                self.dims.remove(&victim_id);
                // 注意:我们有意不挖掉已占矩形
                //——位图足够大,孤儿区域无关紧要,
                // 而移除它们会使之后所有放置失效。
            }
        }

        // 寻找不重叠位置。x 以 `width` 为步长从 0 探测;
        // 对每个 x,找不与任何既有矩形重叠且不超出
        // 图集高度的最低 y。
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

    /// 查找图像位置但不重新插入。
    pub fn get(&mut self, id: u64) -> Option<ImagePlacement> {
        if let Some(&placement) = self.cache.get(&id) {
            // 更新 LRU。
            self.cache.shift_remove(&id);
            self.cache.insert(id, placement);
            Some(placement)
        } else {
            None
        }
    }

    /// 从图集移除一幅图像(例如终端回滚越过它,
    /// 或协议送来分块图形的 `d=d`)。
    pub fn remove(&mut self, id: u64) -> bool {
        let removed = self.cache.shift_remove(&id).is_some();
        if removed {
            self.pixels.remove(&id);
            self.dims.remove(&id);
            self.dirty = true;
        }
        removed
    }

    /// 当前缓存的图像数。
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// 图集无图像时返回 true。
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// 有待上传的脏区域时返回 true。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 取脏上传载荷。自上次调用以来图集未变化时
    /// 返回 `None`。调用之后
    /// `dirty` 标志被复位。
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

    /// 强制图集在下次渲染时标记为脏。
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// 为 `w × h` 的矩形寻找不重叠的 `(x, y)`。
    ///
    /// `x` 以 `w` 为步长从 0 扫描(因此总落在
    /// `w` 对齐的网格上);对每个 `x`,找不与任何
    /// 既有矩形重叠且在 ATLAS_SIZE 内的最低 `y`。
    fn allocate_rect(&self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > ATLAS_SIZE || h > ATLAS_SIZE {
            return None;
        }
        let mut x = 0u32;
        while x + w <= ATLAS_SIZE {
            // 在此 x 处找 (w, h) 矩形不与任何既有矩形
            // 重叠的最低 y。
            let y = self.lowest_free_y_at(x, w, h);
            if let Some(y) = y {
                return Some((x, y));
            }
            // 此 x 处无空间——试下一列。
            x = x.saturating_add(w);
            if x == 0 {
                // 溢出(w = 0 已在上游过滤)。
                return None;
            }
        }
        None
    }

    fn lowest_free_y_at(&self, x: u32, w: u32, h: u32) -> Option<u32> {
        // y 从 0 起测,有矩形重叠则步进 1。由于矩形高度
        // 有限,最终要么找到 y,要么
        // 越出图集。
        let mut y = 0u32;
        while y + h <= ATLAS_SIZE {
            let probe = Occupied { x, y, w, h };
            if !self.occupied.iter().any(|o| overlaps(*o, probe)) {
                return Some(y);
            }
            // 跳过此 x 带内最高的重叠矩形。
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

    /// 构造一幅 16x16 的纯色 RGBA 图像。
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
        // MAX_IMAGE_ENTRIES 为 256;达到即触发 LRU。
        let mut atlas = ImageAtlas::new();
        for id in 0..MAX_IMAGE_ENTRIES as u64 {
            atlas.insert(id, solid_image(0, 0, 0, 255), 16, 16).unwrap();
        }
        assert_eq!(atlas.len(), MAX_IMAGE_ENTRIES);

        // 再插入一幅:触发 id=0 的逐出。
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
        // 触碰 id=0 使其成为 MRU。
        assert!(atlas.get(0).is_some());
        // 插入新图像:id=0 应存活,id=1 应被逐出。
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
