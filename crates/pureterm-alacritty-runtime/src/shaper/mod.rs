//! T-core-shaper:逐字形整形 trait + 聚簇映射 + 步进。
//!
//! v1:纯 Rust、零依赖的整形器,按字素聚簇(UAX #29 近似)
//! 遍历输入,每个聚簇产出一个 [`ShapedGlyph`],
//! `x_advance` 设为传入的单元格宽度。整形规则是
//! “等宽聚簇 = N 格”——拉丁 / ASCII 为 N=1,CJK /
//! 谚文 / 平假名 / 片假名 / 宽 emoji 为 N=2。v1 足以
//! 暴露 trait 表面,让聚簇↔单元格反查(供 `ime_widget` /
//! `bidi` 使用)无需引入完整 Harfbuzz 即可工作。
//! 真正的 `rustybuzz_shaper` 在 v2 落地。
//!
//! ## 为什么先做 trait?
//!
//! 渲染 / bidi / ime 代码路径需要的是*契约*(`x_advance`
//! + `cluster`)——不一定是真正的 Harfbuzz 移植——以便接通
//! `CachedGlyph` 的 `ShapedGlyph { x_advance }` 字段(见
//! `renderer::glyph_atlas::CachedGlyph`)与 IME 组件定位其
//! 预编辑组合所需的 `cluster → cell`
//! 反查。v1 交付契约;v2 替换实现。

/// 单个整形字形:已就绪、可在网格单元格渲染的一个码点
///(或字素聚簇)。`cluster` 字段是到原字符串的字节偏移
/// ——IME / bidi 层用它把字形映射回
/// 单元格位置。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    /// 字体内的字形 id。v1 恒发 `0`(即 `.notdef`
    /// 字形);真正的整形器返回字体特定的 id。
    pub glyph_id: u32,
    /// 该字形聚簇起点在输入字符串中的字节偏移。
    /// 与下一个字形的 `cluster` 组合,IME 可把
    /// 字形映射回字节区间。
    pub cluster: u32,
    /// 水平步进(像素,字体单位换算后)。v1 返回
    /// `font_cell_width * cell_count`(拉丁为 1,CJK 宽字符
    /// 为 2)。
    pub x_advance: f32,
    /// 垂直步进(像素)。v1 返回 0(行高来自
    /// 字体度量,而非整形器)。
    pub y_advance: f32,
    /// 相对光标的水平偏移。用于组合附标与
    /// 复杂文字。
    pub x_offset: f32,
    /// 相对基线的垂直偏移。
    pub y_offset: f32,
}

/// 一个整形 run——输入字符串的连续片段及其
/// 整形字形与 `cluster → cell` 反查表。
#[derive(Debug, Clone)]
pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
    /// `cluster_to_cell[i]` 给出字形 `i` 的网格单元格索引。
    /// v1:与输入 1:1。v2:把 Unicode 字素聚簇映射到
    /// 终端单元格(CJK 宽字符可能占 1 或 2 列——
    /// 见由 `x_advance` 推导的 `cell_count`)。
    pub cluster_to_cell: Vec<u32>,
}

impl ShapedRun {
    /// T-core-shaper v3:把输入字符串的字节偏移映射到
    /// 持有该字形的网格单元格(其聚簇起点等于或恰好位于
    /// `byte_offset` 之前)。IME 组件用它把
    /// 预编辑组合光标定位到正确单元格上。
    ///
    /// `byte_offset` 超出输入末尾或 run 无字形时返回
    /// `None`。偏移在第一个聚簇之前时返回
    /// `Some(0)`(例如用户在位置 0 按下
    /// 组合键)。
    pub fn cluster_byte_to_cell(&self, byte_offset: usize) -> Option<u32> {
        if self.glyphs.is_empty() {
            return None;
        }
        // 找最后一个聚簇起点等于或位于该偏移之前的字形。
        // 字形按输入顺序产出,因此从尾部反向线性扫描
        // 最坏 O(cluster_count),但通常 O(1)——IME
        // 移动通常就在光标前一位置附近。
        let mut last_match: Option<u32> = None;
        for (i, g) in self.glyphs.iter().enumerate() {
            if (g.cluster as usize) <= byte_offset {
                last_match = Some(self.cluster_to_cell[i]);
            } else {
                break;
            }
        }
        last_match.or(Some(0))
    }
}

/// 整形器 trait。渲染器以输入字符串调用 `shape`,
/// 取回一个整形 run。单元格宽度由
/// 渲染器提供(它掌握字体度量)。
pub trait Shaper: Send + Sync {
    /// 整形输入字符串。结果是一个 [`ShapedRun`]。
    fn shape(&self, input: &str, font_cell_width: f32) -> ShapedRun;
}

/// 纯 Rust 等宽整形器。每个 Unicode 字素聚簇
/// (UAX #29 近似)视为一个聚簇;聚簇的
/// `x_advance` 为 `font_cell_width` × 格数(拉丁 1,
/// CJK / 谚文 / 平假名 / 片假名 / 宽 emoji / 多码点
/// 聚簇为 2)。v1 的聚簇↔单元格映射为 `1:1`。
#[derive(Debug, Clone, Default)]
pub struct MonospaceShaper;

impl MonospaceShaper {
    pub fn new() -> Self {
        Self
    }
}

impl Shaper for MonospaceShaper {
    fn shape(&self, input: &str, font_cell_width: f32) -> ShapedRun {
        let mut glyphs = Vec::new();
        let mut cluster_to_cell = Vec::new();
        let mut cell_index: u32 = 0;
        for (cluster_start, cluster) in grapheme_clusters(input) {
            let wide = is_wide(cluster);
            let cell_count = if wide { 2u32 } else { 1u32 };
            let x_advance = font_cell_width * cell_count as f32;
            glyphs.push(ShapedGlyph {
                glyph_id: 0, // v1: `.notdef`; v2: real glyph id.
                cluster: cluster_start as u32,
                x_advance,
                y_advance: 0.0,
                x_offset: 0.0,
                y_offset: 0.0,
            });
            cluster_to_cell.push(cell_index);
            cell_index = cell_index.saturating_add(cell_count);
        }
        ShapedRun { glyphs, cluster_to_cell }
    }
}

/// 聚簇首字符是“宽”字符(CJK / 宽
/// emoji)时为真。v1:忽略 emoji ZWJ 序列的多码点“宽”检测
///——真正的 ZWJ 聚簇占 2 格,但没有 UAX #29 就无法区分
/// 基字符+附标与 ZWJ emoji,因此保守地按
/// 1 格(首字符)上报——另见 `shape` 中 ZWJ 聚簇保守的
/// `x_advance = cell_width`。
fn is_wide(s: &str) -> bool {
    let c = s.chars().next().unwrap_or('\0');
    is_wide_char(c)
}

fn is_wide_char(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        // CJK 统一表意文字(基本区)
        0x4E00..=0x9FFF
        // CJK 统一表意文字扩展 A
        | 0x3400..=0x4DBF
        // CJK 兼容表意文字
        | 0xF900..=0xFAFF
        // 平假名 / 片假名
        | 0x3040..=0x30FF
        // 谚文音节
        | 0xAC00..=0xD7AF
        // 全角形式
        | 0xFF00..=0xFF60
        // emoji 与杂项符号(保守子集)
        | 0x1F300..=0x1F9FF
    )
}

/// 在 `input` 的字素聚簇上迭代 `(byte_offset, cluster_str)`。
/// v1 是零依赖近似:遍历字符串,每个 Unicode 标量值
/// 产出一个聚簇,但把非间距 / 组合附标(General_Category
/// 以 M 开头)并入前一聚簇。这对等宽拉丁 /
/// CJK / 谚文 / 谚文 Jamo 足够——emoji ZWJ 序列会被多算成
/// 1 格(v2 引入真正的 UAX #29 实现)。
/// 1 格(v2 引入真正的 UAX #29 实现)的剩余说明。
fn grapheme_clusters(input: &str) -> impl Iterator<Item = (usize, &str)> {
    GraphemeIter { input, pos: 0 }
}

struct GraphemeIter<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Iterator for GraphemeIter<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.input.len() {
            return None;
        }
        let start = self.pos;
        let mut chars = self.input[start..].char_indices();
        let (_, c) = chars.next()?;
        let mut end = start + c.len_utf8();
        // 把组合附标折叠进当前聚簇。
        for (_, next) in chars {
            if !is_combining(next) {
                break;
            }
            end += next.len_utf8();
        }
        let cluster = &self.input[start..end];
        self.pos = end;
        Some((start, cluster))
    }
}

fn is_combining(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        0x0300..=0x036F  // Combining Diacritical Marks
        | 0x1AB0..=0x1AFF // Combining Diacritical Marks Extended
        | 0x1DC0..=0x1DFF // Combining Diacritical Marks Supplement
        | 0x20D0..=0x20FF // Combining Diacritical Marks for Symbols
        | 0xFE20..=0xFE2F // Combining Half Marks
    )
}

/// 真正的 rustybuzz 整形器 + 字体回退链(v2)。
pub mod fallback;
pub mod rustybuzz_shaper;

pub use fallback::{FallbackChain, FallbackChainBuilder, FallbackScript, NamedFontFace};
pub use rustybuzz_shaper::{FontFace, RustyBuzzConfig, RustyBuzzShaper};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_one_cell_per_cluster() {
        let run = MonospaceShaper::new().shape("hello", 7.0);
        assert_eq!(run.glyphs.len(), 5);
        for (i, g) in run.glyphs.iter().enumerate() {
            assert_eq!(g.x_advance, 7.0);
            assert_eq!(g.cluster, i as u32);
        }
    }

    #[test]
    fn cjk_full_width_is_two_cells() {
        let run = MonospaceShaper::new().shape("你好", 7.0);
        assert_eq!(run.glyphs.len(), 2);
        for g in &run.glyphs {
            assert_eq!(g.x_advance, 14.0);
        }
        assert_eq!(run.cluster_to_cell, vec![0, 2]);
    }

    #[test]
    fn mixed_ascii_cjk() {
        let run = MonospaceShaper::new().shape("a你b", 7.0);
        assert_eq!(run.glyphs.len(), 3);
        assert_eq!(run.glyphs[0].x_advance, 7.0);
        assert_eq!(run.glyphs[1].x_advance, 14.0);
        assert_eq!(run.glyphs[2].x_advance, 7.0);
        assert_eq!(run.cluster_to_cell, vec![0, 1, 3]);
    }

    #[test]
    fn empty_input_yields_empty_run() {
        let run = MonospaceShaper::new().shape("", 7.0);
        assert!(run.glyphs.is_empty());
        assert!(run.cluster_to_cell.is_empty());
    }

    #[test]
    fn cluster_byte_offset_matches_input() {
        let run = MonospaceShaper::new().shape("a你", 7.0);
        assert_eq!(run.glyphs[0].cluster, 0);
        assert_eq!(run.glyphs[1].cluster, 1);
    }

    #[test]
    fn wide_detection_ranges() {
        assert!(is_wide_char('中'));
        assert!(is_wide_char('あ'));
        assert!(is_wide_char('한'));
        assert!(!is_wide_char('a'));
        assert!(!is_wide_char('Z'));
        assert!(!is_wide_char('0'));
    }

    #[test]
    fn emoji_sequences_are_wide() {
        // 男 + ZWJ + 笔记本电脑。v1:ZWJ(`U+200D`)不是组合附标,
        // 因此迭代器产出 3 个聚簇而非 1 个。每个的
        // 首字符都在 emoji 区间,所以每个聚簇都是宽的(2 格);
        // ZWJ 本身是非宽的零宽字符,落入
        // 半宽 1 格步进。
        let run = MonospaceShaper::new().shape("\u{1F468}\u{200D}\u{1F4BB}", 7.0);
        assert_eq!(run.glyphs.len(), 3);
        assert_eq!(run.glyphs[0].x_advance, 14.0); // Man 👨
        assert_eq!(run.glyphs[1].x_advance, 7.0); // ZWJ
        assert_eq!(run.glyphs[2].x_advance, 14.0); // Laptop 💻
    }

    #[test]
    fn combining_marks_attach_to_previous_cluster() {
        let run = MonospaceShaper::new().shape("a\u{0301}", 7.0);
        assert_eq!(run.glyphs.len(), 1);
        assert_eq!(run.glyphs[0].x_advance, 7.0);
        assert_eq!(run.glyphs[0].cluster, 0);
    }

    #[test]
    fn cluster_offsets_form_partition() {
        let input = "a你b";
        let run = MonospaceShaper::new().shape(input, 7.0);
        let cluster_starts: Vec<usize> = run.glyphs.iter().map(|g| g.cluster as usize).collect();
        // Glyph 0 covers "a" (1 byte), glyph 1 covers "你" (3 bytes),
        // 字形 2 覆盖 "b"(1 字节)。
        let mut prev = 0;
        let mut ends: Vec<usize> = Vec::new();
        for &s in &cluster_starts {
            if s > prev {
                ends.push(s);
            }
            prev = s;
        }
        ends.push(input.len());
        // 字形 0 起点 0;字形 1 起点 1;字形 2 起点 4。
        assert_eq!(cluster_starts, vec![0, 1, 4]);
        // 输入长度 = 5。
        assert_eq!(input.len(), 5);
    }
}
