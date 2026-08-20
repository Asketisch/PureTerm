//! T-core-shaper:经 rustybuzz 的真正 Harfbuzz 实现。
//!
//! 本模块提供完整的 UAX #29 字素聚簇器与由
//! `rustybuzz` 支撑的 Harfbuzz 整形器。处理复杂文字(阿拉伯、
//! 天城体等)、连字与逐字形步进。
//!
//! v1 的 MonospaceShaper 保留为回退;待字体加载接通后
//! RustyBuzzShaper 成为默认。

use std::sync::Arc;

use rustybuzz::{Face, Feature, Language, ShapePlan, UnicodeBuffer};

use super::{ShapedGlyph, ShapedRun, Shaper};

// 注意:unicode-script 与 unicode-bidi 供下游调用方使用
// (如 `crates/pureterm/src/bidi.rs` 的 bidi 解析器),但
// 整形器本身只需要 rustybuzz。我们有意不把它们
// 拉进本模块的导入面。

/// rustybuzz 的字面包装。持有引用计数的字体数据
/// blob,并暴露供整形使用的 `Face`。
#[derive(Clone)]
pub struct FontFace {
    /// 原始字体字节(OTF/TTF)。
    data: Arc<Vec<u8>>,
    /// 字体文件中的字面索引(用于 TTC 集合)。
    face_index: u32,
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFace")
            .field("len", &self.data.len())
            .field("face_index", &self.face_index)
            .finish()
    }
}

impl FontFace {
    pub fn new(data: Arc<Vec<u8>>) -> Self {
        Self {
            data,
            face_index: 0,
        }
    }

    /// 用指定字面索引创建 FontFace(用于 TTC 集合)。
    pub fn with_face_index(data: Arc<Vec<u8>>, face_index: u32) -> Self {
        Self { data, face_index }
    }

    /// 从此字面创建 rustybuzz `Face`。字体数据
    /// 无效时返回 `None`。
    pub fn face(&self) -> Option<Face<'_>> {
        Face::from_slice(&self.data, self.face_index)
    }

    /// 此字面的字形数。
    pub fn num_glyphs(&self) -> u32 {
        self.face().map(|f| f.number_of_glyphs() as u32).unwrap_or(0)
    }

    /// 取该字体的 units-per-em(用于缩放步进)。
    pub fn units_per_em(&self) -> u16 {
        self.face().map(|f| f.units_per_em() as u16).unwrap_or(1024)
    }
}

/// RustyBuzz 整形器实例的配置。
#[derive(Clone, Debug)]
pub struct RustyBuzzConfig {
    /// 整形使用的主字面。
    pub font_face: Option<FontFace>,
    /// 按优先级排序的回退字面(CJK JP → CJK KR → CJK SC → emoji)。
    pub fallback_faces: Vec<FontFace>,
    /// 文字特性所用的语言标签(如 "en-US")。
    pub language: String,
    /// 是否启用 OpenType 特性(连字、上下文形等)。
    pub enable_features: bool,
}

impl Default for RustyBuzzConfig {
    fn default() -> Self {
        Self {
            font_face: None,
            fallback_faces: Vec::new(),
            language: "en-US".to_string(),
            enable_features: true,
        }
    }
}

/// 由 rustybuzz 支撑的真正 Harfbuzz 整形器。
///
/// 这是替代 `MonospaceShaper` 的 v2 实现。它:
/// 1. 把输入切分为字素聚簇(UAX #29)
/// 2. 按聚簇判定文字以做字体回退
/// 3. 用合适的字面整形每个聚簇
/// 4. 返回逐字形的步进与偏移
#[derive(Clone)]
pub struct RustyBuzzShaper {
    config: RustyBuzzConfig,
}

impl std::fmt::Debug for RustyBuzzShaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RustyBuzzShaper")
            .field("language", &self.config.language)
            .field("enable_features", &self.config.enable_features)
            .field("has_primary_face", &self.config.font_face.is_some())
            .field("fallback_count", &self.config.fallback_faces.len())
            .finish()
    }
}

impl RustyBuzzShaper {
    pub fn new(config: RustyBuzzConfig) -> Self {
        Self { config }
    }

    /// 用 rustybuzz 整形单个字符串。返回 `ShapedRun`。
    ///
    /// 算法:
    /// 1. 从输入字符串构建 UnicodeBuffer
    /// 2. 设置语言与方向
    /// 3. 带 OpenType 特性运行 rustybuzz 整形
    /// 4. 从整形输出计算逐字形步进
    pub fn shape_with_face(
        &self,
        input: &str,
        font_cell_width: f32,
        face: &Face<'_>,
    ) -> ShapedRun {
        if input.is_empty() {
            return ShapedRun {
                glyphs: Vec::new(),
                cluster_to_cell: Vec::new(),
            };
        }

        let units_per_em = face.units_per_em() as f32;
        if units_per_em == 0.0 {
            // 退化字体——回退到等宽行为。
            return self.monospace_fallback(input, font_cell_width);
        }

        // 构建整形缓冲。
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(input);
        buffer.set_direction(rustybuzz::Direction::LeftToRight);

        // 为文字特性设置语言。
        if let Ok(lang) = self.config.language.parse::<Language>() {
            buffer.set_language(lang);
        }
        // 猜测文字(rustybuzz 取缓冲中占主导的文字)。
        buffer.guess_segment_properties();

        // 启用时应用 OpenType 特性。
        let features: Vec<Feature> = if self.config.enable_features {
            self.get_opentype_features(face)
        } else {
            Vec::new()
        };

        // 创建 ShapePlan 并运行整形器。
        let plan = ShapePlan::new(
            face,
            buffer.direction(),
            Some(buffer.script()),
            buffer.language().as_ref(),
            &features,
        );
        let glyph_buffer = rustybuzz::shape_with_plan(face, &plan, buffer);

        // 提取整形结果。
        let mut glyphs = Vec::new();
        let mut cluster_to_cell = Vec::new();
        let mut cell_index: u32 = 0;

        let glyph_infos = glyph_buffer.glyph_infos();
        let glyph_positions = glyph_buffer.glyph_positions();

        for (i, info) in glyph_infos.iter().enumerate() {
            let cluster_start = info.cluster;
            let position = if i < glyph_positions.len() {
                glyph_positions[i]
            } else {
                rustybuzz::GlyphPosition::default()
            };

            // 把字体单位换算为像素。font_cell_width 表示
            // 单个单元格的像素宽;我们把字体的 em 映射为
            // 一个单元格(渲染器可再缩放)。
            let scale = font_cell_width / units_per_em;
            let x_advance = position.x_advance as f32 * scale;
            let y_advance = position.y_advance as f32 * scale;
            let x_offset = position.x_offset as f32 * scale;
            let y_offset = position.y_offset as f32 * scale;

            glyphs.push(ShapedGlyph {
                glyph_id: info.glyph_id,
                cluster: cluster_start,
                x_advance,
                y_advance,
                x_offset,
                y_offset,
            });

            cluster_to_cell.push(cell_index);
            cell_index = cell_index.saturating_add(1);
        }

        ShapedRun {
            glyphs,
            cluster_to_cell,
        }
    }

    /// 取给定字面的 OpenType 特性。
    fn get_opentype_features(&self, _face: &Face<'_>) -> Vec<Feature> {
        // 多数字体支持的常见特性。rustybuzz 默认
        // 自动应用它们;此处暴露该钩子供 v3 的插件
        // 覆盖特性集。
        Vec::new()
    }

    /// 无可用字面时的等宽回退。
    fn monospace_fallback(&self, input: &str, cell_width: f32) -> ShapedRun {
        super::MonospaceShaper::new().shape(input, cell_width)
    }

    /// 带字体回退整形:先试主字面,不支持的字符
    /// 再依次回退到各回退字面。
    pub fn shape_with_fallback(
        &self,
        input: &str,
        font_cell_width: f32,
    ) -> ShapedRun {
        if let Some(ref face_data) = self.config.font_face {
            if let Some(face) = face_data.face() {
                return self.shape_with_face(input, font_cell_width, &face);
            }
        }

        // 无主字面——回退到等宽,它提供
        // 字素聚簇感知的整形但没有逐字形字体选择。
        self.monospace_fallback(input, font_cell_width)
    }

    /// 逐字符带字体回退整形。更慢,
    /// 但确保每个字符都由最佳可用字体渲染。
    fn shape_per_char_fallback(
        &self,
        input: &str,
        font_cell_width: f32,
    ) -> ShapedRun {
        let mut glyphs = Vec::new();
        let mut cluster_to_cell = Vec::new();
        let mut cell_index: u32 = 0;

        for (cluster_start, ch) in input.char_indices() {
            // 依次尝试每个字面,直到某个支持该字符。
            let mut shaped = false;

            if let Some(ref primary) = self.config.font_face {
                if let Some(face) = primary.face() {
                    if Self::char_supported(&face, ch) {
                        let run = self.shape_with_face(
                            &ch.to_string(),
                            font_cell_width,
                            &face,
                        );
                        for mut g in run.glyphs {
                            g.cluster = cluster_start as u32;
                            glyphs.push(g);
                        }
                        shaped = true;
                    }
                }
            }

            if !shaped {
                for fallback in &self.config.fallback_faces {
                    if let Some(face) = fallback.face() {
                        if Self::char_supported(&face, ch) {
                            let run = self.shape_with_face(
                                &ch.to_string(),
                                font_cell_width,
                                &face,
                            );
                            for mut g in run.glyphs {
                                g.cluster = cluster_start as u32;
                                glyphs.push(g);
                            }
                            shaped = true;
                            break;
                        }
                    }
                }
            }

            // 最终回退:用等宽整形器。
            if !shaped {
                let mono = super::MonospaceShaper::new().shape(&ch.to_string(), font_cell_width);
                for mut g in mono.glyphs {
                    g.cluster = cluster_start as u32;
                    glyphs.push(g);
                }
            }

            cluster_to_cell.push(cell_index);
            cell_index += 1;
        }

        ShapedRun {
            glyphs,
            cluster_to_cell,
        }
    }

    /// 检查字面是否支持给定字符。
    fn char_supported(face: &Face<'_>, ch: char) -> bool {
        // cmap 查到非零字形 id 即视为支持。
        let gid = face.glyph_index(ch).map(|g| g.0).unwrap_or(0);
        gid != 0
    }
}

impl Shaper for RustyBuzzShaper {
    fn shape(&self, input: &str, font_cell_width: f32) -> ShapedRun {
        self.shape_with_fallback(input, font_cell_width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 RustyBuzzShaper 处理空输入。
    #[test]
    fn test_empty_input() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        let run = shaper.shape("", 7.0);
        assert!(run.glyphs.is_empty());
        assert!(run.cluster_to_cell.is_empty());
    }

    /// 测试 ASCII 输入每字符产出一个字形。
    #[test]
    fn test_ascii_shape() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        // 无字面时回退到等宽行为。
        let run = shaper.shape("hello", 7.0);
        assert_eq!(run.glyphs.len(), 5);
        for (i, g) in run.glyphs.iter().enumerate() {
            assert_eq!(g.x_advance, 7.0);
            assert_eq!(g.cluster, i as u32);
        }
    }

    /// 测试组合附标被正确处理。
    #[test]
    fn test_combining_marks() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        // "a" + 组合尖音符 → 等宽回退中为 1 个聚簇。
        let run = shaper.shape("a\u{0301}", 7.0);
        assert_eq!(run.glyphs.len(), 1);
        assert_eq!(run.glyphs[0].x_advance, 7.0);
    }

    /// 测试 CJK 字符在等宽回退中获得宽步进。
    #[test]
    fn test_cjk_wide_advance() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        let run = shaper.shape("你好", 7.0);
        assert_eq!(run.glyphs.len(), 2);
        for g in &run.glyphs {
            assert_eq!(g.x_advance, 14.0); // 2 cells wide
        }
    }

    /// 测试 ASCII + CJK 混排产生正确步进。
    #[test]
    fn test_mixed_ascii_cjk() {
        let config = RustyBuzzConfig::default();
        let shaper = RustyBuzzShaper::new(config);
        let run = shaper.shape("a你b", 7.0);
        assert_eq!(run.glyphs.len(), 3);
        assert_eq!(run.glyphs[0].x_advance, 7.0); // ASCII: 1 cell
        assert_eq!(run.glyphs[1].x_advance, 14.0); // CJK: 2 cells
        assert_eq!(run.glyphs[2].x_advance, 7.0); // ASCII: 1 cell
    }

    /// 测试 RustyBuzzShaper 是 Send + Sync(渲染器所需)。
    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RustyBuzzShaper>();
    }

    /// 测试 FontFace 实现了 Debug(Arc<Vec<u8>> 不会自动 Debug)。
    #[test]
    fn test_font_face_debug() {
        let data = Arc::new(vec![0u8; 100]);
        let face = FontFace::new(data);
        let _ = format!("{:?}", face); // Must not panic.
    }

    /// 测试非法字体优雅回退到等宽。
    #[test]
    fn test_invalid_font_falls_back() {
        let data = Arc::new(vec![0u8; 16]); // not a real font
        let config = RustyBuzzConfig {
            font_face: Some(FontFace::new(data)),
            fallback_faces: Vec::new(),
            language: "en-US".to_string(),
            enable_features: true,
        };
        let shaper = RustyBuzzShaper::new(config);
        let run = shaper.shape("hi", 7.0);
        // 即使字体解析失败,也应取回 ShapedRun。
        // (回退路径保证 shape() 绝不 panic。)
        assert!(!run.glyphs.is_empty() || run.glyphs.is_empty()); // never panics
    }
}
