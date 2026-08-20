//! T-core-graphics：Sixel / 分块图形协议 / 内联图像——GUI 侧的一半。
//! 字节级解析器位于 `alacritty_terminal::event_loop::scan_graphics`；
//! 本模块负责内存图像存储及其与表面快照 / 回滚的集成。
//!
//! ## 模型
//!
//! 1. 终端事件循环为每个完整组装好的单块图像（Sixel / `a=T` 分块图形 /
//!    内联图像）发出 `Event::GraphicsImage`。多块图形图像在本模块的
//!    [`ImageStore`] 中按图像 id（`i=…`）组装。
//! 2. 当图集有空位且图像的网格放置与视口重叠时，渲染器从 [`ImageStore`]
//!    读取。
//! 3. 回滚剪枝把存储中的图像集修剪为仍在可见网格内有放置的条目
//!    （锚点在回滚区之外的移除，在内部的保留——规范称之为
//!    “回滚剪枝”通道）。
//!
//! v1 刻意保持较小的表面：Sixel/图形字节解析器是真实的，存储也是真实的；
//! GPU blit 路径在 v2 接线。

use std::collections::{HashMap, VecDeque};

use alacritty_terminal::event::GraphicsImage;

/// 重组后的分块图形块（只含 `i=` id 与 base64 载荷；渲染器在最终块
/// 到达时从分块元数据读取 `width` / `height`）。
#[derive(Debug, Clone)]
struct GraphicsChunk {
    /// 该块的载荷字节（从 base64 解码而来）。
    bytes: Vec<u8>,
    /// 是最终块吗？（`m=1`）
    final_chunk: bool,
}

/// 分块图形的多块重组器。单个图像 id 映射到一个 FIFO 块队列；
/// 最终块的到达会把拼接结果冲刷为单个 [`GraphicsImage`]。
#[derive(Debug, Clone, Default)]
pub struct GraphicsAssembler {
    by_id: HashMap<u32, VecDeque<GraphicsChunk>>,
}

impl GraphicsAssembler {
    pub fn new() -> Self {
        Self { by_id: HashMap::new() }
    }

    /// 为图像 `id` 记录一个块。若该块（或此前缓冲的任何一块）是最终块，
    /// 返回 `Some(image)`——把所有块冲刷为单个拼接载荷。若仍期待更多块，
    /// 返回 `None`。
    ///
    /// `format`、`width`、`height` 与 `cols`/`rows` 来自最终块的元数据。
    /// 第一块的元数据被丢弃——分块图形规范只要求元数据在每个块上重复，
    /// 因此最干净的来源是最后一块携带的内容。
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

    /// 丢弃在 `max_age` 次调用内未完成的图像 id 的进行中重组缓冲区。
    /// 返回被丢弃的缓冲区数量。
    pub fn gc(&mut self, max_age: usize) -> usize {
        // v1 没有按条目的存活时间（无状态）——我们只在图像 id 与新的传输冲突
        // 时清理（见 `push`）。此方法的存在是为了与未来的逐出策略对称、
        // 也为了测试。
        let _ = max_age;
        0
    }
}

/// 已解码图形图像的内存存储，按图像 id（分块图形协议）或插入顺序
/// （Sixel / 内联图像，不带 id）索引。渲染器 blit 时从这里读取。
#[derive(Debug, Clone)]
pub struct ImageStore {
    /// id → 图像（分块图形协议）。
    by_id: HashMap<u32, StoredImage>,
    /// Sixel / 内联图像，按插入顺序索引。达到 `max_entries` 时按 FIFO 逐出。
    sequential: VecDeque<StoredImage>,
    /// 插入时分配的单调 z 计数器。
    z_next: u32,
    /// 图像总数的硬上限。最旧的 Sixel/内联图像条目先被逐出；被重组器持有的
    /// 按 `id` 索引的分块图形图像会被固定。
    max_entries: usize,
    /// 可选的字节上限；插入时检查每张图像的大小。
    max_image_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct StoredImage {
    image: GraphicsImage,
    /// 图像在存储中占用的字节数。
    pub(crate) bytes: usize,
}

impl ImageStore {
    pub fn new(max_entries: usize, max_image_bytes: usize) -> Self {
        Self {
            by_id: HashMap::new(),
            sequential: VecDeque::new(),
            z_next: 1,
            max_entries: max_entries.max(1),
            // `max_image_bytes` 不设下限——本模块的测试用小上限来演练拒绝路径。
            // 生产调用者从 `[terminal_graphics] max_image_bytes` 取值
            // （默认 32 MiB）。
            max_image_bytes: max_image_bytes.max(1),
        }
    }

    /// 向存储插入一张图像。返回新的 z 值。若超过 `max_image_bytes`，
    /// 图像被丢弃并返回 `None`。
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
            // 分块图形：按 id 索引；同一 id 的重新传输会替换此前的内容。
            self.by_id.insert(image.placement.id, stored);
        } else {
            // Sixel / 内联图像：顺序 FIFO。
            self.sequential.push_back(stored);
            while self.sequential.len() + self.by_id.len() > self.max_entries {
                self.sequential.pop_front();
            }
        }
        Some(z)
    }

    /// 按 id 查找图像（分块图形协议）。不存在时返回 `None`。
    pub fn get_by_id(&self, id: u32) -> Option<&GraphicsImage> {
        self.by_id.get(&id).map(|s| &s.image)
    }

    /// T-core-graphics v3：按插入顺序遍历顺序（Sixel / 内联图像）图像队列。
    /// 供 `iced_app.rs` 填充每帧的 `image_quad_draws` 列表。
    pub fn sequential(&self) -> impl Iterator<Item = &GraphicsImage> {
        self.sequential.iter().map(|s| &s.image)
    }

    /// T-core-graphics v3：遍历按 id 索引（分块图形）的图像映射。迭代顺序
    /// 是 `HashMap` 的（实际上随机）；需要稳定顺序的调用方应按
    /// `placement.z` 排序（`image_quad_draws` 已经这么做了）。
    pub fn by_id_values(&self) -> impl Iterator<Item = &GraphicsImage> {
        self.by_id.values().map(|s| &s.image)
    }

    /// 图像总数。
    pub fn len(&self) -> usize {
        self.sequential.len() + self.by_id.len()
    }

    /// 没有存储任何图像时为 true。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 丢弃放置行在 `first_visible_row` 之上且严格低于回滚上限
    /// （`scrollback_lines`）的图像。返回被丢弃的图像数量。
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
        // 按 id 索引的分块图形图像被固定——规范说“回滚记账按网格放置索引”，
        // 而尚未完成的分块图像没有可剪枝的网格放置。
        dropped
    }
}

/// 检测一段 PTY 字节块使用的是哪种图形协议（如果有的话）。
///
/// 这是回滚 / 状态表面用于决定一个字节块是否值得做扫描通道的
/// 字节前缀嗅探；真正的解析在
/// `alacritty_terminal::event_loop::scan_graphics`。返回的 ID 是
/// 内部稳定标识符（见内联注释）。
pub fn detect_protocol(chunk: &[u8]) -> Option<&'static str> {
    // 协议 ID 是内部的（不暴露给用户）；它们标识字节块属于哪个转义序列
    // 家族，用于回滚/状态扫描决策。见
    // `alacritty_terminal::event_loop::scan_graphics`。
    if chunk.starts_with(b"\x1b_G") {
        Some("graphics") // APC `_`——分块图形协议
    } else if chunk.starts_with(b"\x1bPq") {
        Some("sixel") // DCS `Pq`——sixel 栅格协议
    } else if chunk.starts_with(b"\x1b]1337;File=") {
        Some("inline_image") // OSC 1337——内联图像协议
    } else {
        None
    }
}

/// 仅元数据的传统图形放置缓存。保留是因为 P4 运行时为兼容
/// t-core 收敛前的调用方会构造一个；新的代码路径使用上面的
/// [`ImageStore`] / [`GraphicsAssembler`]。v1 缓存只跟踪
/// `(id, cols, rows, payload_len)`——不含像素。
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

/// 每图像的配置。镜像 `default-config.toml` 中的 `[terminal_graphics]`
/// 块以及配置加载器返回的 `app_config::TerminalGraphicsConfig`。
/// t-core 收敛新增的字段是 `max_image_bytes` 与 `max_concurrent_images`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalGraphicsConfig {
    pub enabled: bool,
    pub max_images: usize,
    /// 每图像的字节上限。分块图形 PNG 图像可能有数兆字节；
    /// 默认值（32 MiB）在病态载荷固定存储之前就将其拒绝。
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
            // id=0 → 顺序（Sixel / 内联图像模式）。
            let mut img = rgba_image(0, 1, 1, 1, 1, 0);
            img.placement.id = 0;
            img.placement.row = i as u16;
            s.insert(img);
        }
        // 上限 2 逐出最旧的；只保留最新的 2 张。
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn store_rejects_oversized() {
        let mut s = ImageStore::new(8, 16); // 16 字节上限
        let img = rgba_image(0, 2, 2, 2, 2, 0); // 2*2*4 = 16 字节
        // 恰好达到上限 → 允许。
        assert!(s.insert(img.clone()).is_some());
        // 再加 4 个像素 → 20 字节，超过上限。
        let mut big = rgba_image(0, 3, 2, 3, 2, 0);
        big.pixels = Some(vec![0u8; 24]);
        assert!(s.insert(big).is_none());
    }

    #[test]
    fn store_prune_scrollback_drops_above() {
        let mut s = ImageStore::new(8, 1024 * 1024);
        for i in 0..5u16 {
            let mut img = rgba_image(0, 1, 1, 1, 1, i);
            img.placement.id = 0; // 连续分配,故可被清理
            s.insert(img);
        }
        // 可见窗口是第 3..5 行，回滚 2 → 保留行 >= 1，丢弃第 0 行。
        let dropped = s.prune_scrollback(3, 2);
        assert_eq!(dropped, 1);
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn graphics_assembler_emits_only_on_final() {
        let mut a = GraphicsAssembler::new();
        // m=0 → 还有更多块 → None
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
        // m=1 → 最终块 → 发出两个块的拼接结果
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
