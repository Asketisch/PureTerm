//! 字形光栅化器 + GPU 字形 atlas 状态([`GlyphState`])。
//!
//! 每个 pane 共享一个 `GlyphState`(由 `TerminalProgram` 持有 `Arc<RefCell<…>>`),
//! 在 `draw()` 中光栅化/缓存字形并通过 `WgpuGlyphAtlas` 暴露脏区数据,
//! 供 `TerminalPrimitive::prepare()` 上传到 GPU。

use std::sync::Arc;

use crossfont::{FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Style, Weight, Slant};
use pureterm_alacritty_runtime::renderer::glyph_atlas::{CachedGlyph, WgpuGlyphAtlas};
use pureterm_alacritty_runtime::shaper::{ShapedRun, Shaper};

pub struct GlyphState {
    pub atlas: WgpuGlyphAtlas,
    pub rasterizer: Rasterizer,
    pub font_key: FontKey,
    pub font_size: Size,
    /// 实际光栅化尺寸(逻辑尺寸 × scale_factor),用于 HiDPI 支持。
    pub scaled_font_size: Size,
    pub cell_width: f32,
    pub cell_height: f32,
    pub font_descent: f32,
    pub scale_factor: f32,
    /// 保存字体配置,重配置时无需调用方再次提供。
    pub font_family: String,
    pub size_pt: f32,
    /// T-core-shaper v2:可插拔的整形器(默认 Monospace,一旦宿主
    /// 通过 `AppConfig::shaper` 接入真实字体数据,即可替换为
    /// `RustyBuzzShaper`)。
    pub shaper: Arc<dyn Shaper>,
}

impl GlyphState {
    pub fn new_with_config(font_family: &str, size_pt: f32, scale_factor: f32) -> Self {
        let mut rasterizer = Rasterizer::new().expect("Failed to create rasterizer");
        let font_size = Size::new(size_pt);
        let scaled_font_size = font_size.scale(scale_factor);
        let font_desc = FontDesc::new(font_family, Style::Description { slant: Slant::Normal, weight: Weight::Normal });
        let font_key = rasterizer.load_font(&font_desc, scaled_font_size).unwrap_or_else(|_| {
            let fallback = FontDesc::new("monospace", Style::Description { slant: Slant::Normal, weight: Weight::Normal });
            rasterizer.load_font(&fallback, scaled_font_size).expect("Failed to load fallback font")
        });
        let metrics = rasterizer.metrics(font_key, scaled_font_size).expect("Failed to get metrics");

        // 把物理度量换算回逻辑坐标,用于网格计算
        let cell_width = (metrics.average_advance / scale_factor as f64).floor().max(1.0) as f32;
        let cell_height = (metrics.line_height / scale_factor as f64).floor().max(1.0) as f32;
        let font_descent = metrics.descent / scale_factor as f32;

        Self {
            atlas: WgpuGlyphAtlas::new(),
            rasterizer,
            font_key,
            font_size,
            scaled_font_size,
            cell_width,
            cell_height,
            font_descent,
            scale_factor,
            font_family: font_family.to_string(),
            size_pt,
            // T-core-shaper:v1 使用等宽簇整形器;v2 在配置
            // `[shaper] backend = "rustybuzz"` 时切换为 `RustyBuzzShaper`
            // (见 `app_config.rs` B.4 配置说明)。
            shaper: Arc::new(pureterm_alacritty_runtime::shaper::MonospaceShaper::new()),
        }
    }

    /// T-core-shaper v2:安装自定义整形器(例如用真实字体数据
    /// 构造的 `RustyBuzzShaper`)。
    pub fn set_shaper(&mut self, shaper: Arc<dyn Shaper>) {
        self.shaper = shaper;
    }

    /// T-core-shaper v2:用当前整形器处理字符串,返回
    /// 各字形的推进量,调用方可据此移动光标。
    pub fn shape_text(&self, text: &str) -> ShapedRun {
        self.shaper.shape(text, self.cell_width)
    }

    /// 同时重新配置字体族、字号与缩放因子。
    pub fn reconfigure_scaled(&mut self, font_family: &str, size_pt: f32, scale_factor: f32) {
        self.scale_factor = scale_factor;
        self.font_family = font_family.to_string();
        self.size_pt = size_pt;
        let font_size = Size::new(size_pt);
        let scaled_font_size = font_size.scale(scale_factor);
        let font_desc = FontDesc::new(font_family, Style::Description { slant: Slant::Normal, weight: Weight::Normal });
        let font_key = self.rasterizer.load_font(&font_desc, scaled_font_size).unwrap_or_else(|_| {
            let fallback = FontDesc::new("monospace", Style::Description { slant: Slant::Normal, weight: Weight::Normal });
            self.rasterizer.load_font(&fallback, scaled_font_size).expect("Failed to load fallback font")
        });
        let metrics = self.rasterizer.metrics(font_key, scaled_font_size).expect("Failed to get metrics");

        // 把物理度量换算回逻辑坐标,用于网格计算
        self.font_key = font_key;
        self.font_size = font_size;
        self.scaled_font_size = scaled_font_size;
        self.cell_width = (metrics.average_advance / scale_factor as f64).floor().max(1.0) as f32;
        self.cell_height = (metrics.line_height / scale_factor as f64).floor().max(1.0) as f32;
        self.font_descent = metrics.descent / scale_factor as f32;
        self.atlas = WgpuGlyphAtlas::new();
    }

    /// 仅更新缩放因子,保留当前字体族与字号。
    /// 若缩放因子确实发生变化则返回 true。
    pub fn update_scale_factor(&mut self, scale_factor: f32) -> bool {
        if (self.scale_factor - scale_factor).abs() < 0.01 {
            return false;
        }
        self.reconfigure_scaled(&self.font_family.clone(), self.size_pt, scale_factor);
        true
    }

    pub fn get_or_rasterize(&mut self, c: char) -> Option<CachedGlyph> {
        if let Some(cached) = self.atlas.get(0, c) {
            // Atlas 存储物理像素字形;顶点定位需按比例换算回逻辑坐标,
            // 命中缓存时必须与下方新光栅化路径
            // 采用相同的归一化,否则在 Retina(scale_factor=2.0)上
            // 消费方会得到 2 倍于
            // 预期的逻辑宽度,字形将溢出所在单元格。
            return Some(CachedGlyph {
                u: cached.u,
                v: cached.v,
                u2: cached.u2,
                v2: cached.v2,
                x_offset: cached.x_offset / self.scale_factor,
                y_offset: cached.y_offset / self.scale_factor,
                width: cached.width / self.scale_factor,
                height: cached.height / self.scale_factor,
                x_advance: cached.x_advance / self.scale_factor,
                y_advance: cached.y_advance / self.scale_factor,
                atlas_kind: cached.atlas_kind,
            });
        }

        let glyph_key = GlyphKey { character: c, font_key: self.font_key, size: self.scaled_font_size };
        match self.rasterizer.get_glyph(glyph_key) {
            Ok(glyph) => {
                let cached = self.atlas.insert(0, c, &glyph);
                // Atlas 存储物理像素字形;顶点定位时按比例换算回逻辑坐标
                Some(CachedGlyph {
                    u: cached.u,
                    v: cached.v,
                    u2: cached.u2,
                    v2: cached.v2,
                    x_offset: cached.x_offset / self.scale_factor,
                    y_offset: cached.y_offset / self.scale_factor,
                    width: cached.width / self.scale_factor,
                    height: cached.height / self.scale_factor,
                    x_advance: cached.x_advance / self.scale_factor,
                    y_advance: cached.y_advance / self.scale_factor,
                    atlas_kind: cached.atlas_kind,
                })
            }
            Err(_) => None,
        }
    }

    pub fn mark_atlas_dirty(&mut self) {
        self.atlas.mark_dirty();
    }
}
