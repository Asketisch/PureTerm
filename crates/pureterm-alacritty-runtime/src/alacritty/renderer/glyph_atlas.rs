//! Wgpu 字形图集——把 crossfont 的 RasterizedGlyph 桥接到 CPU 侧纹理图集。
//!
//! 双图集设计:文本字形用灰度 R8 图集,彩色 emoji 用
//! RGBA 图集。天际线装箱把字形放进固定的 2048x2048 位图。
//! 图集装满时逐出 LRU 条目并重建两个图集。

use crossfont::{BitmapBuffer, RasterizedGlyph};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

/// 字形位于哪个图集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasKind {
    Grayscale,
    Color,
}

/// 已缓存字形在图集中的位置。
#[derive(Clone, Copy, Debug)]
pub struct CachedGlyph {
    /// 图集 UV 空间中左上角的 U 坐标。
    pub u: f32,
    /// 图集 UV 空间中左上角的 V 坐标。
    pub v: f32,
    /// 图集 UV 空间中右下角的 U 坐标。
    pub u2: f32,
    /// 图集 UV 空间中右下角的 V 坐标。
    pub v2: f32,
    /// 相对单元格原点的水平偏移(像素)。
    pub x_offset: f32,
    /// 相对单元格原点的垂直偏移(像素)。
    pub y_offset: f32,
    /// 字形宽度(像素)。
    pub width: f32,
    /// 字形高度(像素)。
    pub height: f32,
    /// 水平步进(像素,整形后)。v1 调用方把它留在
    /// `0.0`;渲染器应用等宽单元格宽度,v2 中
    /// 整形层可按字形覆盖。
    pub x_advance: f32,
    /// 垂直步进(像素,整形后)。v1 调用方把它留在
    /// `0.0`;竖排文字(CJK 侧躺)会填充它。
    pub y_advance: f32,
    /// 该字形位于哪个图集。
    pub atlas_kind: AtlasKind,
}

/// 天际线装箱段。
#[derive(Clone, Debug)]
struct SkylineSeg {
    x: u32,
    y: u32,
    width: u32,
}

/// 图集尺寸。
const ATLAS_SIZE: u32 = 2048;

/// 触发逐出前可缓存字形的最大数量。
const MAX_CACHE_ENTRIES: usize = 8192;

/// 自上次上传以来图集中发生变化的一个矩形区域。
///
/// 供渲染线程做部分 `queue.write_texture` 调用,
/// 而不是在只碰到单个字形的每一帧重传整个
/// 4MB+16MB 图集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 渲染线程上传已变化图集区域所需的数据。
///
/// `data` 是完整图集(一整块 `Arc<Vec<u8>>`);渲染
/// 线程按 `TexelCopyBufferLayout` 的字节偏移切出子矩形。返回 `Arc` 让
/// 主线程与渲染线程共享分配,无需 20MB 克隆。
/// 主线程与渲染线程共享同一分配,无需 20MB 克隆。
pub struct AtlasDirty {
    pub regions: Vec<DirtyRect>,
    pub data: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// WgpuGlyphAtlas——带天际线装箱的 CPU 侧字形缓存。
///
/// 双图集:文本用灰度 R8(4 MB),彩色 emoji 用 RGBA(16 MB)。
/// 装满时逐出 LRU 条目并重建。
pub struct WgpuGlyphAtlas {
    atlas_width: u32,
    atlas_height: u32,
    /// R8 位图数据(每像素一字节)。
    atlas_data: Vec<u8>,
    /// 灰度装箱的天际线段。
    skyline: Vec<SkylineSeg>,
    /// 字形缓存:(font_index, character) -> 图集位置。
    cache: HashMap<(usize, char), CachedGlyph>,
    /// 为重建而保存的光栅化字形。
    stored_glyphs: HashMap<(usize, char), RasterizedGlyph>,
    /// 由 `IndexMap` 支撑的 LRU 顺序:前 = 最旧,后 = 最新。
    /// PureTerm:逐出用 O(1) 的 `shift_remove_index(0)`(此前是 O(n) 的
    /// `Vec::position` + `Vec::remove`)。
    lru_keys: IndexMap<(usize, char), ()>,
    /// 缓存条目上限。
    capacity: usize,
    /// 灰度图集位图自上次 GPU 上传以来是否变化过。
    dirty: bool,
    /// 自上次 `take_dirty_regions` 以来灰度变化的逐矩形跟踪。
    /// 供渲染线程做部分上传。
    dirty_regions: Vec<DirtyRect>,
    /// RGBA 彩色图集位图(每像素 4 字节)。
    color_atlas_data: Vec<u8>,
    /// 彩色图集装箱的天际线段。
    color_skyline: Vec<SkylineSeg>,
    /// 彩色图集位图自上次 GPU 上传以来是否变化过。
    color_dirty: bool,
    /// 自上次以来彩色图集变化的逐矩形
    /// `take_color_dirty_regions`.
    color_dirty_regions: Vec<DirtyRect>,
}

impl WgpuGlyphAtlas {
    /// 创建新的空字形图集。
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

    /// 尝试取已缓存字形。未缓存返回 `None`。
    /// 命中时把键移到 LRU 列表尾部。
    pub fn get(&mut self, font_index: usize, c: char) -> Option<CachedGlyph> {
        let key = (font_index, c);
        if let Some(&glyph) = self.cache.get(&key) {
            // 移到 LRU 末尾(最近使用)。IndexMap 下 O(1)。
            self.lru_keys.shift_remove(&key);
            self.lru_keys.insert(key, ());
            Some(glyph)
        } else {
            None
        }
    }

    /// 把 crossfont 的光栅化字形插入图集。
    ///
    /// 把 `BitmapBuffer::Rgba` 字形路由到彩色图集,其余
    /// 路由到灰度图集。返回带 UV 坐标的 `CachedGlyph`。
    /// 图集已满时触发重建(逐出一半 LRU 条目)。
    pub fn insert(&mut self, font_index: usize, c: char, glyph: &RasterizedGlyph) -> CachedGlyph {
        let key = (font_index, c);

        // 已缓存则更新 LRU 并返回。IndexMap 下 O(1)。
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

        // 保存光栅化字形以备可能的重建。
        self.stored_glyphs.insert(key, Self::clone_rasterized(glyph));
        self.cache.insert(key, cached);
        self.lru_keys.insert(key, ());

        cached
    }

    /// 把灰度字形插入 R8 图集。
    fn insert_grayscale(
        &mut self,
        _font_index: usize,
        _c: char,
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
                x_advance: 0.0,
                y_advance: 0.0,
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
            x_advance: 0.0,
            y_advance: 0.0,
            atlas_kind: AtlasKind::Grayscale,
        }
    }

    /// 把彩色(RGBA)字形插入彩色图集。
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
                x_advance: 0.0,
                y_advance: 0.0,
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
            x_advance: 0.0,
            y_advance: 0.0,
            atlas_kind: AtlasKind::Color,
        }
    }

    /// 检查图集纹理是否需要重新上传。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 强制灰度与彩色图集纹理在下次渲染时重新上传。
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.color_dirty = true;
    }

    /// 取图集位图数据并标记为干净。返回 `(data, width, height)`。
    pub fn take_dirty_data(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        if self.dirty {
            self.dirty = false;
            Some((self.atlas_data.clone(), self.atlas_width, self.atlas_height))
        } else {
            None
        }
    }

    /// 取脏区域 + 完整图集缓冲,并把图集标记为
    /// 干净。图集干净时返回 `None`。
    ///
    /// `data` 是 `Arc<Vec<u8>>`,使主线程与渲染
    /// 线程都能持有缓冲而无需额外克隆。完整图集
    ///(而非仅脏子矩形)被返回,因为 GPU
    /// 上传 API 需要带行距的 `&[u8]` 切片;渲染
    /// 线程用每个区域的 `(x, y)` 作为 `TexelCopyTextureInfo`
    /// 原点,以及此缓冲内的字节偏移。
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

    /// 取图集位图数据用于上传,但不标记干净。
    pub fn atlas_data(&self) -> &[u8] {
        &self.atlas_data
    }

    /// 图集尺寸,形如 `(width, height)`。
    pub fn atlas_dimensions(&self) -> (u32, u32) {
        (self.atlas_width, self.atlas_height)
    }

    /// 检查彩色图集纹理是否需要重新上传。
    pub fn is_color_dirty(&self) -> bool {
        self.color_dirty
    }

    /// 取彩色图集位图数据并标记干净。返回 `(data, width, height)`。
    pub fn take_color_dirty_data(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        if self.color_dirty {
            self.color_dirty = false;
            Some((self.color_atlas_data.clone(), self.atlas_width, self.atlas_height))
        } else {
            None
        }
    }

    /// [`take_dirty_regions`] 的彩色图集对应版本。
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

    // ---- 私有辅助 ----

    /// 从 crossfont `RasterizedGlyph` 提取灰度(R8)位图。
    ///
    /// 只处理 `BitmapBuffer::Rgb`。返回 `(width, height, grayscale_data)`。
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
                // 回退:把 alpha 提取为灰度蒙版。
                rgba.chunks_exact(4).map(|px| px[3]).collect()
            },
        };

        (w, h, gray)
    }

    /// 从彩色字形提取 RGBA 像素数据。
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

    /// 克隆一份 RasterizedGlyph 用于存储(重建时需要)。
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

    /// 把灰度字形块传(blit)到图集位图的 `(x, y)` 处。
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

    /// 把 RGBA 彩色字形块传到彩色图集的 `(x, y)` 处。
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

    /// 用天际线装箱在彩色图集中分配一个矩形。
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
        let i = best_idx + 1;
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

    /// 用天际线装箱在图集中分配一个矩形。
    ///
    /// 返回 `(x, y)` 左上位置,无空间时返回 `None`。
    fn allocate_rect(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width == 0 || height == 0 || width > self.atlas_width || height > self.atlas_height {
            return None;
        }

        let mut best_y = u32::MAX;
        let mut best_idx = 0;

        // 寻找能给出最低结果 Y 的天际线位置。
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

        // 为已放置的矩形插入新的天际线节点。
        let new_seg = SkylineSeg {
            x: best_x,
            y: best_y,
            width,
        };

        self.skyline.insert(best_idx, new_seg);

        // 裁剪或移除被新矩形覆盖的段。
        let right_edge = best_x + width;
        let i = best_idx + 1;
        while i < self.skyline.len() {
            let seg_end = self.skyline[i].x + self.skyline[i].width;
            if self.skyline[i].x < right_edge {
                if seg_end <= right_edge {
                    // 完全覆盖——移除。
                    self.skyline.remove(i);
                } else {
                    // 部分覆盖——从左侧收缩。
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

    /// 检查 `width` x `height` 的矩形能否从天际线段 `idx` 起放下。
    /// 返回所放矩形顶部的 Y 坐标,放不下返回 None。
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

    /// 合并同一 Y 层上相邻的天际线段。
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

    /// 逐出后重建两个图集。
    ///
    /// 逐出 LRU 中较旧的一半并重新装箱剩余字形。
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

        // PureTerm:用 `drain(..evict_count)` 以 O(n) 移除最旧的一半
        // (单次分配搬移)。语义与旧的 `Vec::drain`
        // 路径等价,但与新的 IndexMap 保持一致。
        let evicted: Vec<_> = self.lru_keys.drain(..evict_count).collect();
        for (key, _) in &evicted {
            self.cache.remove(key);
            self.stored_glyphs.remove(key);
        }

        // 重置两个图集的位图与天际线。
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
                        x_advance: 0.0,
                        y_advance: 0.0,
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
                        x_advance: 0.0,
                        y_advance: 0.0,
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
        // v2:x_advance 与 y_advance 默认为 0;整形器 / 渲染器
        // 负责从真实整形过程填充它们。
        assert_eq!(cached.x_advance, 0.0);
        assert_eq!(cached.y_advance, 0.0);
    }

    /// v2:手工构造的 `CachedGlyph`(例如整形器在产出
    /// 整形 run 时)必须保留逐字形的 `x_advance`。
    /// 该测试防止结构体重构时不慎剥离字段。
    /// 该测试防止结构体重构时不慎剥离字段。
    #[test]
    fn test_cached_glyph_preserves_x_advance() {
        let g = CachedGlyph {
            u: 0.0,
            v: 0.0,
            u2: 0.25,
            v2: 0.5,
            x_offset: 0.0,
            y_offset: 0.0,
            width: 8.0,
            height: 14.0,
            x_advance: 12.0,
            y_advance: 0.0,
            atlas_kind: AtlasKind::Grayscale,
        };
        assert_eq!(g.x_advance, 12.0);
        assert_eq!(g.y_advance, 0.0);
    }

    /// v2:CJK 宽字符的 `CachedGlyph` 必须携带 2 格步进。
    #[test]
    fn test_cached_glyph_cjk_advance() {
        let g = CachedGlyph {
            u: 0.0,
            v: 0.0,
            u2: 0.0,
            v2: 0.0,
            x_offset: 0.0,
            y_offset: 0.0,
            width: 14.0,
            height: 14.0,
            x_advance: 14.0, // 2 cells at 7 px each
            y_advance: 0.0,
            atlas_kind: AtlasKind::Grayscale,
        };
        // 单元格宽 7 时,2 格宽 = 14.0。
        assert_eq!(g.x_advance, 14.0);
    }

    #[test]
    fn test_insert_color_glyph() {
        let mut atlas = WgpuGlyphAtlas::new();
        // 模拟彩色 emoji 字形(RGBA)。
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
        // 第二次调用应返回 None。
        assert!(atlas.take_dirty_data().is_none());
    }

    /// 辅助:构造一个 1x1 的 Rgb 小字形。
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
        // PureTerm:IndexMap 支撑的 LRU——命中时 `get` 必须把
        // 条目移到尾部,使下一次逐出放过它。
        let mut atlas = WgpuGlyphAtlas::new();
        atlas.insert(0, 'A', &tiny_glyph('A'));
        atlas.insert(0, 'B', &tiny_glyph('B'));
        atlas.insert(0, 'C', &tiny_glyph('C'));

        // 触碰 'A',使其成为最近使用。
        assert!(atlas.get(0, 'A').is_some());

        // 此刻 LRU 顺序应为:B、C、A。
        let order: Vec<_> = atlas.lru_keys.keys().copied().collect();
        assert_eq!(order, vec![(0, 'B'), (0, 'C'), (0, 'A')]);
    }

    #[test]
    fn lru_insert_existing_promotes_to_back() {
        let mut atlas = WgpuGlyphAtlas::new();
        atlas.insert(0, 'A', &tiny_glyph('A'));
        atlas.insert(0, 'B', &tiny_glyph('B'));
        atlas.insert(0, 'A', &tiny_glyph('A')); // re-insert 'A'

        // 重新插入后,A 应位于尾部。
        let order: Vec<_> = atlas.lru_keys.keys().copied().collect();
        assert_eq!(order, vec![(0, 'B'), (0, 'A')]);
    }

    #[test]
    fn lru_drain_evicts_oldest() {
        // PureTerm:重建路径用 `drain(..evict_count)` 以 O(n) 移除
        // 最旧的一半(单次分配搬移)。
        let mut atlas = WgpuGlyphAtlas::new();
        for c in "ABCDE".chars() {
            atlas.insert(0, c, &tiny_glyph(c));
        }
        // 顺序:A B C D E(A 最旧)。
        let evicted: Vec<_> = atlas.lru_keys.drain(..2).collect();
        let evicted_keys: Vec<_> = evicted.into_iter().map(|(k, ())| k).collect();
        assert_eq!(evicted_keys, vec![(0, 'A'), (0, 'B')]);
        // 幸存者按序:C D E。
        let survivors: Vec<_> = atlas.lru_keys.keys().copied().collect();
        assert_eq!(survivors, vec![(0, 'C'), (0, 'D'), (0, 'E')]);
    }

    /// PureTerm B3.1:`take_dirty_regions` 返回逐字形区域,
    /// 而不是迫使渲染线程重传整个图集。单个
    /// 新字形(8192 个缓存槽之一)产生一个小矩形。
    #[test]
    fn dirty_regions_track_per_glyph() {
        let mut atlas = WgpuGlyphAtlas::new();
        atlas.insert(0, 'A', &tiny_glyph('A'));
        atlas.insert(0, 'B', &tiny_glyph('B'));
        let dirty = atlas.take_dirty_regions().expect("atlas is dirty");
        assert_eq!(dirty.regions.len(), 2);
        // 每个区域都是 1x1(tiny_glyph)。
        for r in &dirty.regions {
            assert_eq!(r.w, 1);
            assert_eq!(r.h, 1);
        }
        // 取走后图集变干净,第二次取返回 None。
        assert!(atlas.take_dirty_regions().is_none());
        // Arc<data> 共享完整图集缓冲(4MB)。
        assert_eq!(dirty.data.len(), (ATLAS_SIZE * ATLAS_SIZE) as usize);
    }

    /// PureTerm B3.1:`take_color_dirty_regions` 是彩色图集的
    /// 对应版本。
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
        // 彩色图集每像素 4 字节。
        assert_eq!(dirty.data.len(), (ATLAS_SIZE * ATLAS_SIZE * 4) as usize);
    }
}
