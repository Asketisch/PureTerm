//! 终端特定字符的 Sprite 字体绘制。
//!
//! 此模块为通常渲染为块图形的终端特定字符提供自定义绘制。
//! 不依赖字体光栅化，我们直接使用简单的几何原语绘制这些字符。
//!
//! 支持的字符范围：
//! - 块元素 (U+2580-U+259F)
//! - 框绘制 (U+2500-U+257F)
//! - 盲文模式 (U+2800-U+28FF)
//! - Powerline 符号 (U+E0B0-U+E0BF)
//!
//! 参考实现：
//! - Ghostty: src/font/sprite/draw/block.zig, box.zig, braille.zig, powerline.zig
//! - iTerm2: iTermBoxDrawingBezierCurveFactory.m

use crate::fonts::GlyphInfo;

/// 终端单元格的 Sprite 绘制指标。
#[derive(Debug, Clone)]
pub struct SpriteMetrics {
    /// 单元格宽度（像素）
    pub cell_width: u32,
    /// 单元格高度（像素）
    pub cell_height: u32,
    /// 下延（正值向下）
    pub descent: f32,
    /// 上延（正值向上）
    pub ascent: f32,
}

impl SpriteMetrics {
    pub fn new(cell_width: u32, cell_height: u32, font_size: f32) -> Self {
        // 典型终端指标：上延约为字号的 80%，下延约为 20%
        let ascent = font_size * 0.8;
        let descent = font_size * 0.2;
        Self {
            cell_width,
            cell_height,
            descent,
            ascent,
        }
    }
}

/// 块元素的阴影级别。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shade {
    Light = 0x40,    // 25%
    Medium = 0x80,   // 50%
    Dark = 0xC0,     // 75%
    On = 0xFF,       // 100%
}

impl Shade {
    pub fn to_alpha(self) -> u8 {
        self as u8
    }
}

/// 将填充的矩形绘制到图集数据中。
#[inline]
pub fn draw_rect(
    atlas_data: &mut [u8],
    atlas_width: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    shade: Shade,
) {
    let alpha = shade.to_alpha();
    for py in 0..height as i32 {
        for px in 0..width as i32 {
            let ax = x + px;
            let ay = y + py;
            if ax >= 0 && ax < atlas_width as i32 && ay >= 0 {
                let idx = (ay as usize) * atlas_width as usize + ax as usize;
                if idx < atlas_data.len() {
                    atlas_data[idx] = alpha;
                }
            }
        }
    }
}

/// 填充单元格的一个象限。
#[inline]
fn fill_quadrant(
    atlas_data: &mut [u8],
    atlas_width: u32,
    metrics: &SpriteMetrics,
    offset_x: u32,
    offset_y: u32,
    tl: bool,
    tr: bool,
    bl: bool,
    br: bool,
) {
    let half_w = metrics.cell_width / 2;
    let half_h = metrics.cell_height / 2;
    let right_w = metrics.cell_width - half_w;
    let bottom_h = metrics.cell_height - half_h;

    if tl {
        draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, half_w, half_h, Shade::On);
    }
    if tr {
        draw_rect(atlas_data, atlas_width, (offset_x + half_w) as i32, offset_y as i32, right_w, half_h, Shade::On);
    }
    if bl {
        draw_rect(atlas_data, atlas_width, offset_x as i32, (offset_y + half_h) as i32, half_w, bottom_h, Shade::On);
    }
    if br {
        draw_rect(atlas_data, atlas_width, (offset_x + half_w) as i32, (offset_y + half_h) as i32, right_w, bottom_h, Shade::On);
    }
}

/// 绘制半块（水平或垂直分割）。
#[allow(dead_code)]
#[inline]
fn draw_half_block(
    atlas_data: &mut [u8],
    atlas_width: u32,
    metrics: &SpriteMetrics,
    offset_x: u32,
    offset_y: u32,
    horizontal: bool,
    shade: Shade,
) {
    if horizontal {
        // 上半部分（填充上半部分）
        let half_h = metrics.cell_height / 2;
        draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, metrics.cell_width, half_h, shade);
    } else {
        // 右半部分填充
        let half_w = metrics.cell_width / 2;
        draw_rect(atlas_data, atlas_width, (offset_x + half_w) as i32, offset_y as i32, metrics.cell_width - half_w, metrics.cell_height, shade);
    }
}

/// 检查字符是否为块元素 (U+2580-U+259F)。
#[inline]
pub fn is_block_element(c: char) -> bool {
    let code = c as u32;
    (0x2580..=0x259F).contains(&code)
}

/// 终端渲染的光标样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    /// 块光标（填充矩形）：█
    Block,
    /// 下划线光标（水平线）：▁
    Underline,
    /// 横梁光标（垂直条）：▏
    Beam,
    /// 条形光标（垂直线，类似于下划线但位于字符边缘）
    Bar,
}

impl CursorStyle {
    /// 在给定位置绘制光标 sprite。
    pub fn draw(
        &self,
        atlas_data: &mut [u8],
        atlas_width: u32,
        _atlas_height: u32,
        metrics: &SpriteMetrics,
        cursor_x: &mut u32,
        cursor_y: &mut u32,
    ) -> Option<GlyphInfo> {
        let width = metrics.cell_width;
        let height = metrics.cell_height;
        let offset_x = *cursor_x;
        let offset_y = *cursor_y;

        match self {
            CursorStyle::Block => {
                // 块光标 - 使用现有的块元素绘制
                return draw_block_element(atlas_data, atlas_width, metrics, '█', cursor_x, cursor_y);
            }
            CursorStyle::Underline => {
                // 下划线光标 - 底部细水平线
                let thickness = std::cmp::max(1, height / 10);
                let y = offset_y + height - thickness;
                draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y as i32, width, thickness);
            }
            CursorStyle::Beam => {
                // 横梁光标 - 左侧细垂直条
                let thickness = std::cmp::max(1, width / 10);
                draw_vertical_line(atlas_data, atlas_width, offset_x as i32, offset_y as i32, height, thickness);
            }
            CursorStyle::Bar => {
                // 条形光标 - 字符右边缘的细垂直条
                let thickness = std::cmp::max(1, width / 10);
                let x = offset_x as i32 + (width as i32 - thickness as i32);
                draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height, thickness);
            }
        }

        // 推进光标
        *cursor_x += width + 1;

        Some(GlyphInfo {
            x: offset_x,
            y: offset_y,
            width: width as u16,
            height: height as u16,
            x_offset: 0.0,
            y_offset: 0.0,
            advance: width as f32,
            is_ligature: false,
            ligature_len: 1,
        })
    }
}

/// 检查字符是否为框绘制字符 (U+2500-U+257F)。
#[inline]
pub fn is_box_drawing(c: char) -> bool {
    let code = c as u32;
    (0x2500..=0x257F).contains(&code)
}

/// 检查字符是否为盲文模式 (U+2800-U+28FF)。
#[inline]
pub fn is_braille(c: char) -> bool {
    let code = c as u32;
    (0x2800..=0x28FF).contains(&code)
}

/// 检查字符是否为 Powerline 符号 (U+E0B0-U+E0BF)。
#[inline]
pub fn is_powerline(c: char) -> bool {
    let code = c as u32;
    (0xE0B0..=0xE0BF).contains(&code)
}

/// 将块元素字符绘制到图集数据中。
/// 如果字符被绘制则返回 Some(GlyphInfo)，如果不支持则返回 None。
///
/// 此函数将字符直接绘制到图集中当前光标位置，并更新光标位置。
pub fn draw_block_element(
    atlas_data: &mut [u8],
    atlas_width: u32,
    metrics: &SpriteMetrics,
    c: char,
    cursor_x: &mut u32,
    cursor_y: &mut u32,
) -> Option<GlyphInfo> {
    let code = c as u32;
    let width = metrics.cell_width;
    let height = metrics.cell_height;
    let offset_x = *cursor_x;
    let offset_y = *cursor_y;

    match code {
        // ▀ U+2580: UPPER HALF BLOCK
        0x2580 => {
            let half_h = height / 2;
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, width, height - half_h, Shade::On);
        }

        // ▁ U+2581: LOWER ONE EIGHTH BLOCK
        0x2581 => {
            let h = height / 8;
            draw_rect(atlas_data, atlas_width, offset_x as i32, (offset_y + height - h) as i32, width, h, Shade::On);
        }

        // ▂ U+2582: LOWER ONE QUARTER BLOCK
        0x2582 => {
            let h = height / 4;
            draw_rect(atlas_data, atlas_width, offset_x as i32, (offset_y + height - h) as i32, width, h, Shade::On);
        }

        // ▃ U+2583: LOWER THREE EIGHTHS BLOCK
        0x2583 => {
            let h = (height * 3) / 8;
            draw_rect(atlas_data, atlas_width, offset_x as i32, (offset_y + height - h) as i32, width, h, Shade::On);
        }

        // ▄ U+2584: LOWER HALF BLOCK
        0x2584 => {
            let half_h = height / 2;
            draw_rect(atlas_data, atlas_width, offset_x as i32, (offset_y + half_h) as i32, width, height - half_h, Shade::On);
        }

        // ▅ U+2585: LOWER FIVE EIGHTHS BLOCK
        0x2585 => {
            let h = (height * 5) / 8;
            draw_rect(atlas_data, atlas_width, offset_x as i32, (offset_y + height - h) as i32, width, h, Shade::On);
        }

        // ▆ U+2586: LOWER THREE QUARTERS BLOCK
        0x2586 => {
            let h = (height * 3) / 4;
            draw_rect(atlas_data, atlas_width, offset_x as i32, (offset_y + height - h) as i32, width, h, Shade::On);
        }

        // ▇ U+2587: LOWER SEVEN EIGHTHS BLOCK
        0x2587 => {
            let h = (height * 7) / 8;
            draw_rect(atlas_data, atlas_width, offset_x as i32, (offset_y + height - h) as i32, width, h, Shade::On);
        }

        // █ U+2588: FULL BLOCK
        0x2588 => {
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, width, height, Shade::On);
        }

        // ▉ U+2589: LEFT SEVEN EIGHTHS BLOCK
        0x2589 => {
            let w = (width * 7) / 8;
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, w, height, Shade::On);
        }

        // ▊ U+258A: LEFT THREE QUARTERS BLOCK
        0x258A => {
            let w = (width * 3) / 4;
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, w, height, Shade::On);
        }

        // ▋ U+258B: LEFT FIVE EIGHTHS BLOCK
        0x258B => {
            let w = (width * 5) / 8;
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, w, height, Shade::On);
        }

        // ▌ U+258C: LEFT HALF BLOCK
        0x258C => {
            let w = width / 2;
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, w, height, Shade::On);
        }

        // ▍ U+258D: LEFT THREE EIGHTHS BLOCK
        0x258D => {
            let w = (width * 3) / 8;
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, w, height, Shade::On);
        }

        // ▎ U+258E: LEFT ONE QUARTER BLOCK
        0x258E => {
            let w = width / 4;
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, w, height, Shade::On);
        }

        // ▏ U+258F: LEFT ONE EIGHTH BLOCK
        0x258F => {
            let w = width / 8;
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, w, height, Shade::On);
        }

        // ▐ U+2590: RIGHT HALF BLOCK
        0x2590 => {
            let w = width / 2;
            draw_rect(atlas_data, atlas_width, (offset_x + w) as i32, offset_y as i32, width - w, height, Shade::On);
        }

        // ░ U+2591: 浅色阴影
        0x2591 => {
            // 绘制 25% 密度的棋盘格图案
            let cell_size = 2;
            for py in 0..height {
                for px in 0..width {
                    if ((px / cell_size) + (py / cell_size)) % 2 == 0 {
                        let idx = ((offset_y + py) * atlas_width + offset_x + px) as usize;
                        if idx < atlas_data.len() {
                            atlas_data[idx] = Shade::Light as u8;
                        }
                    }
                }
            }
            // 提前返回 - 已绘制
            *cursor_x += width + 1;
            return Some(GlyphInfo {
                x: offset_x,
                y: offset_y,
                width: width as u16,
                height: height as u16,
                x_offset: 0.0,
                y_offset: 0.0,
                advance: width as f32,
                is_ligature: false,
                ligature_len: 1,
            });
        }

        // ▒ U+2592: MEDIUM SHADE
        0x2592 => {
            let cell_size = 2;
            for py in 0..height {
                for px in 0..width {
                    let idx = ((offset_y + py) * atlas_width + offset_x + px) as usize;
                    if idx < atlas_data.len() {
                        if ((px / cell_size) + (py / cell_size)) % 2 == 0 {
                            atlas_data[idx] = Shade::Medium as u8;
                        } else {
                            atlas_data[idx] = Shade::Light as u8;
                        }
                    }
                }
            }
            *cursor_x += width + 1;
            return Some(GlyphInfo {
                x: offset_x,
                y: offset_y,
                width: width as u16,
                height: height as u16,
                x_offset: 0.0,
                y_offset: 0.0,
                advance: width as f32,
                is_ligature: false,
                ligature_len: 1,
            });
        }

        // ▓ U+2593: DARK SHADE
        0x2593 => {
            let cell_size = 2;
            for py in 0..height {
                for px in 0..width {
                    let idx = ((offset_y + py) * atlas_width + offset_x + px) as usize;
                    if idx < atlas_data.len() {
                        if ((px / cell_size) + (py / cell_size)) % 2 == 0 {
                            atlas_data[idx] = Shade::Dark as u8;
                        } else {
                            atlas_data[idx] = Shade::Medium as u8;
                        }
                    }
                }
            }
            *cursor_x += width + 1;
            return Some(GlyphInfo {
                x: offset_x,
                y: offset_y,
                width: width as u16,
                height: height as u16,
                x_offset: 0.0,
                y_offset: 0.0,
                advance: width as f32,
                is_ligature: false,
                ligature_len: 1,
            });
        }

        // ▔ U+2594: UPPER ONE EIGHTH BLOCK
        0x2594 => {
            let h = height / 8;
            draw_rect(atlas_data, atlas_width, offset_x as i32, offset_y as i32, width, h, Shade::On);
        }

        // ▕ U+2595: RIGHT ONE EIGHTH BLOCK
        0x2595 => {
            let w = width / 8;
            draw_rect(atlas_data, atlas_width, (offset_x + width - w) as i32, offset_y as i32, w, height, Shade::On);
        }

        // ▖ U+2596: QUADRANT LOWER LEFT
        0x2596 => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, false, false, true, false);
        }

        // ▗ U+2597: QUADRANT LOWER RIGHT
        0x2597 => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, false, false, false, true);
        }

        // ▘ U+2598: QUADRANT UPPER LEFT
        0x2598 => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, true, false, false, false);
        }

        // ▙ U+2599: QUADRANT UPPER LEFT AND LOWER LEFT AND LOWER RIGHT
        0x2599 => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, true, false, true, true);
        }

        // ▚ U+259A: QUADRANT UPPER LEFT AND LOWER RIGHT
        0x259A => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, true, false, false, true);
        }

        // ▛ U+259B: QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER LEFT
        0x259B => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, true, true, true, false);
        }

        // ▜ U+259C: QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT
        0x259C => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, true, true, false, true);
        }

        // ▝ U+259D: QUADRANT UPPER RIGHT
        0x259D => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, false, true, false, false);
        }

        // ▞ U+259E: QUADRANT UPPER RIGHT AND LOWER LEFT
        0x259E => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, false, true, true, false);
        }

        // ▟ U+259F: QUADRANT UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT
        0x259F => {
            fill_quadrant(atlas_data, atlas_width, metrics, offset_x, offset_y, false, true, true, true);
        }

        _ => return None,
    }

    // 推进光标
    *cursor_x += width + 1;

    Some(GlyphInfo {
        x: offset_x,
        y: offset_y,
        width: width as u16,
        height: height as u16,
        x_offset: 0.0,
        y_offset: 0.0,
        advance: width as f32,
        is_ligature: false,
        ligature_len: 1,
    })
}

/// 绘制具有适当厚度的水平线段。
#[inline]
fn draw_horizontal_line(
    atlas_data: &mut [u8],
    atlas_width: u32,
    x: i32,
    y: i32,
    width: u32,
    thickness: u32,
) {
    draw_rect(atlas_data, atlas_width, x, y, width, thickness, Shade::On);
}

/// 绘制具有适当厚度的垂直线段。
#[inline]
fn draw_vertical_line(
    atlas_data: &mut [u8],
    atlas_width: u32,
    x: i32,
    y: i32,
    height: u32,
    thickness: u32,
) {
    draw_rect(atlas_data, atlas_width, x, y, thickness, height, Shade::On);
}

/// 参数化框绘制的边缘权重。
/// 0 = 无边缘，1 = 细，2 = 粗。
struct BoxEdges {
    top: u8,
    right: u8,
    bottom: u8,
    left: u8,
}

/// 将框绘制码点 (U+2500-U+257F) 映射到其边缘配置。
/// 范围外的码点返回 None。
fn box_drawing_edges(code: u32) -> Option<BoxEdges> {
    match code {
        // 虚线（近似为实线）
        0x2504..=0x2505 => Some(BoxEdges { top: 0, right: 0, bottom: 0, left: 1 }), // approximated
        0x2506..=0x2507 => Some(BoxEdges { top: 1, right: 0, bottom: 1, left: 0 }), // approximated
        0x2508..=0x2509 => Some(BoxEdges { top: 0, right: 0, bottom: 0, left: 1 }), // approximated
        0x250A..=0x250B => Some(BoxEdges { top: 1, right: 0, bottom: 1, left: 0 }), // approximated

        // Mixed-weight corners (top-left variants)
        // ┎ U+250C is already handled above (light/light)
        0x250E => Some(BoxEdges { top: 0, right: 2, bottom: 2, left: 0 }), // ┎ light/heavy down-right
        0x250F => Some(BoxEdges { top: 0, right: 2, bottom: 2, left: 0 }), // ┏ heavy down-right

        // Mixed-weight corners (top-right variants)
        0x2511 => Some(BoxEdges { top: 0, right: 0, bottom: 2, left: 2 }), // ┑
        0x2512 => Some(BoxEdges { top: 0, right: 0, bottom: 1, left: 2 }), // ┒
        0x2513 => Some(BoxEdges { top: 0, right: 0, bottom: 2, left: 2 }), // ┓

        // Mixed-weight corners (bottom-left variants)
        0x2515 => Some(BoxEdges { top: 2, right: 2, bottom: 0, left: 0 }), // ┕
        0x2516 => Some(BoxEdges { top: 1, right: 2, bottom: 0, left: 0 }), // ┖
        0x2517 => Some(BoxEdges { top: 2, right: 2, bottom: 0, left: 0 }), // ┗

        // Mixed-weight corners (bottom-right variants)
        0x2519 => Some(BoxEdges { top: 2, right: 0, bottom: 0, left: 2 }), // ┙
        0x251A => Some(BoxEdges { top: 2, right: 0, bottom: 0, left: 1 }), // ┚
        0x251B => Some(BoxEdges { top: 2, right: 0, bottom: 0, left: 2 }), // ┛

        // Mixed-weight tee pieces
        0x251D => Some(BoxEdges { top: 1, right: 2, bottom: 1, left: 0 }), // ┝ light vert, heavy right
        0x251E => Some(BoxEdges { top: 1, right: 1, bottom: 2, left: 0 }), // ┞
        0x251F => Some(BoxEdges { top: 2, right: 1, bottom: 1, left: 0 }), // ┟
        0x2520 => Some(BoxEdges { top: 2, right: 2, bottom: 2, left: 0 }), // ┠ heavy
        0x2521 => Some(BoxEdges { top: 2, right: 1, bottom: 2, left: 0 }), // ┡
        0x2522 => Some(BoxEdges { top: 1, right: 2, bottom: 1, left: 0 }), // ┢
        0x2523 => Some(BoxEdges { top: 2, right: 2, bottom: 2, left: 0 }), // ┣ heavy

        0x2525 => Some(BoxEdges { top: 1, right: 0, bottom: 1, left: 2 }), // ┥
        0x2526 => Some(BoxEdges { top: 1, right: 0, bottom: 2, left: 1 }), // ┦
        0x2527 => Some(BoxEdges { top: 2, right: 0, bottom: 1, left: 1 }), // ┧
        0x2528 => Some(BoxEdges { top: 2, right: 0, bottom: 2, left: 2 }), // ┨ heavy
        0x2529 => Some(BoxEdges { top: 2, right: 0, bottom: 2, left: 1 }), // ┩
        0x252A => Some(BoxEdges { top: 1, right: 0, bottom: 1, left: 2 }), // ┪
        0x252B => Some(BoxEdges { top: 2, right: 0, bottom: 2, left: 2 }), // ┫ heavy

        // Mixed-weight tee pieces (horizontal)
        0x252D => Some(BoxEdges { top: 1, right: 1, bottom: 2, left: 1 }), // ┭
        0x252E => Some(BoxEdges { top: 2, right: 1, bottom: 1, left: 1 }), // ┮
        0x252F => Some(BoxEdges { top: 1, right: 2, bottom: 1, left: 1 }), // ┯
        0x2530 => Some(BoxEdges { top: 2, right: 2, bottom: 2, left: 2 }), // ┰ heavy
        0x2531 => Some(BoxEdges { top: 2, right: 1, bottom: 2, left: 1 }), // ┱
        0x2532 => Some(BoxEdges { top: 1, right: 2, bottom: 1, left: 2 }), // ┲
        0x2533 => Some(BoxEdges { top: 2, right: 2, bottom: 2, left: 2 }), // ┳ heavy

        0x2535 => Some(BoxEdges { top: 2, right: 1, bottom: 1, left: 1 }), // ┵
        0x2536 => Some(BoxEdges { top: 1, right: 1, bottom: 2, left: 1 }), // ┶
        0x2537 => Some(BoxEdges { top: 1, right: 1, bottom: 1, left: 2 }), // ┷
        0x2538 => Some(BoxEdges { top: 2, right: 2, bottom: 2, left: 2 }), // ┸ heavy
        0x2539 => Some(BoxEdges { top: 1, right: 1, bottom: 2, left: 2 }), // ┹
        0x253A => Some(BoxEdges { top: 2, right: 2, bottom: 1, left: 1 }), // ┺
        0x253B => Some(BoxEdges { top: 2, right: 2, bottom: 2, left: 2 }), // ┻ heavy

        // Mixed-weight crosses
        0x253D => Some(BoxEdges { top: 1, right: 2, bottom: 1, left: 1 }), // ╽
        0x253E => Some(BoxEdges { top: 1, right: 1, bottom: 1, left: 2 }), // ╾
        0x253F => Some(BoxEdges { top: 1, right: 1, bottom: 1, left: 1 }), // ╿
        0x2540 => Some(BoxEdges { top: 2, right: 1, bottom: 1, left: 1 }), // ╀
        0x2541 => Some(BoxEdges { top: 1, right: 1, bottom: 2, left: 1 }), // ╁
        0x2542 => Some(BoxEdges { top: 2, right: 1, bottom: 2, left: 1 }), // ╂
        0x2543 => Some(BoxEdges { top: 2, right: 2, bottom: 1, left: 1 }), // ╃
        0x2544 => Some(BoxEdges { top: 2, right: 1, bottom: 1, left: 2 }), // ╄
        0x2545 => Some(BoxEdges { top: 1, right: 1, bottom: 2, left: 2 }), // ╅
        0x2546 => Some(BoxEdges { top: 1, right: 2, bottom: 2, left: 1 }), // ╆
        0x2547 => Some(BoxEdges { top: 2, right: 2, bottom: 2, left: 1 }), // ╇
        0x2548 => Some(BoxEdges { top: 2, right: 1, bottom: 2, left: 2 }), // ╈
        0x2549 => Some(BoxEdges { top: 1, right: 2, bottom: 2, left: 2 }), // ╉
        0x254A => Some(BoxEdges { top: 2, right: 2, bottom: 2, left: 2 }), // ╊
        0x254B => Some(BoxEdges { top: 2, right: 2, bottom: 2, left: 2 }), // ╋ heavy cross

        // Arc corners (U+256D-U+2570)
        0x256D => Some(BoxEdges { top: 0, right: 1, bottom: 1, left: 0 }), // ╭ rounded top-left
        0x256E => Some(BoxEdges { top: 0, right: 0, bottom: 1, left: 1 }), // ╮ rounded top-right
        0x256F => Some(BoxEdges { top: 1, right: 0, bottom: 0, left: 1 }), // ╯ rounded bottom-right
        0x2570 => Some(BoxEdges { top: 1, right: 1, bottom: 0, left: 0 }), // ╰ rounded bottom-left

        // One-directional lines
        0x2571 => Some(BoxEdges { top: 0, right: 0, bottom: 0, left: 0 }), // ╱ diagonal (handled separately below)
        0x2572 => Some(BoxEdges { top: 0, right: 0, bottom: 0, left: 0 }), // ╲ diagonal (handled separately)
        0x2573 => Some(BoxEdges { top: 0, right: 0, bottom: 0, left: 0 }), // ╳ cross diagonal
        0x2574 => Some(BoxEdges { top: 0, right: 0, bottom: 0, left: 1 }), // ╴ left
        0x2575 => Some(BoxEdges { top: 1, right: 0, bottom: 0, left: 0 }), // ╵ up
        0x2576 => Some(BoxEdges { top: 0, right: 1, bottom: 0, left: 0 }), // ╶ right
        0x2577 => Some(BoxEdges { top: 0, right: 0, bottom: 1, left: 0 }), // ╷ down
        0x2578 => Some(BoxEdges { top: 0, right: 0, bottom: 0, left: 2 }), // ╸ heavy left
        0x2579 => Some(BoxEdges { top: 2, right: 0, bottom: 0, left: 0 }), // ╹ heavy up
        0x257A => Some(BoxEdges { top: 0, right: 2, bottom: 0, left: 0 }), // ╺ heavy right
        0x257B => Some(BoxEdges { top: 0, right: 0, bottom: 2, left: 0 }), // ╻ heavy down
        0x257C => Some(BoxEdges { top: 0, right: 2, bottom: 0, left: 1 }), // ╼
        0x257D => Some(BoxEdges { top: 1, right: 0, bottom: 0, left: 2 }), // ╽
        0x257E => Some(BoxEdges { top: 0, right: 1, bottom: 0, left: 2 }), // ╾
        0x257F => Some(BoxEdges { top: 0, right: 0, bottom: 2, left: 1 }), // ╿

        _ => None,
    }
}

/// 将框绘制字符绘制到图集数据中。
/// 如果字符被绘制则返回 Some(GlyphInfo)，如果不支持则返回 None。
pub fn draw_box_drawing(
    atlas_data: &mut [u8],
    atlas_width: u32,
    metrics: &SpriteMetrics,
    c: char,
    cursor_x: &mut u32,
    cursor_y: &mut u32,
) -> Option<GlyphInfo> {
    let code = c as u32;
    let width = metrics.cell_width;
    let height = metrics.cell_height;
    let offset_x = *cursor_x;
    let offset_y = *cursor_y;

    // 线宽通常为单元格尺寸的 1/10
    let line_thickness = std::cmp::max(1, width.min(height) / 10);
    let half_thickness = line_thickness / 2;

    match code {
        // ─ U+2500: BOX DRAWINGS LIGHT HORIZONTAL
        0x2500 => {
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width, line_thickness);
        }
        // ━ U+2501: BOX DRAWINGS HEAVY HORIZONTAL
        0x2501 => {
            let thickness = line_thickness * 2;
            let y = offset_y as i32 + (height as i32 - thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width, thickness);
        }
        // │ U+2502: BOX DRAWINGS LIGHT VERTICAL
        0x2502 => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height, line_thickness);
        }
        // ┃ U+2503: BOX DRAWINGS HEAVY VERTICAL
        0x2503 => {
            let thickness = line_thickness * 2;
            let x = offset_x as i32 + (width as i32 - thickness as i32) / 2;
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height, thickness);
        }
        // ┌ U+250C: BOX DRAWINGS LIGHT DOWN AND RIGHT
        0x250C => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width - half_thickness as u32, line_thickness);
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height - half_thickness as u32, line_thickness);
        }
        // ┐ U+2510: BOX DRAWINGS LIGHT DOWN AND LEFT
        0x2510 => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32 + half_thickness as i32, y, width - half_thickness as u32, line_thickness);
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height - half_thickness as u32, line_thickness);
        }
        // └ U+2514: BOX DRAWINGS LIGHT UP AND RIGHT
        0x2514 => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width - half_thickness as u32, line_thickness);
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32 + half_thickness as i32, height - half_thickness as u32, line_thickness);
        }
        // ┘ U+2518: BOX DRAWINGS LIGHT UP AND LEFT
        0x2518 => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32 + half_thickness as i32, y, width - half_thickness as u32, line_thickness);
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32 + half_thickness as i32, height - half_thickness as u32, line_thickness);
        }
        // ├ U+251C: BOX DRAWINGS LIGHT VERTICAL AND RIGHT
        0x251C => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width, line_thickness);
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height, line_thickness);
        }
        // ┤ U+2524: BOX DRAWINGS LIGHT VERTICAL AND LEFT
        0x2524 => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width, line_thickness);
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height, line_thickness);
        }
        // ┬ U+252C: BOX DRAWINGS LIGHT DOWN AND HORIZONTAL
        0x252C => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width, line_thickness);
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height - half_thickness as u32, line_thickness);
        }
        // ┴ U+2534: BOX DRAWINGS LIGHT UP AND HORIZONTAL
        0x2534 => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width, line_thickness);
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32 + half_thickness as i32, height - half_thickness as u32, line_thickness);
        }
        // ┼ U+253C: BOX DRAWINGS LIGHT VERTICAL AND HORIZONTAL
        0x253C => {
            let x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
            let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width, line_thickness);
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height, line_thickness);
        }
        // ═ U+2550: BOX DRAWINGS DOUBLE HORIZONTAL
        0x2550 => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total_height = 2 * thickness + gap;
            let start_y = offset_y as i32 + (height as i32 - total_height as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
        }
        // ║ U+2551: BOX DRAWINGS DOUBLE VERTICAL
        0x2551 => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total_width = 2 * thickness + gap;
            let start_x = offset_x as i32 + (width as i32 - total_width as i32) / 2;
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╔ U+2554: BOX DRAWINGS DOUBLE DOWN AND RIGHT
        0x2554 => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total = 2 * thickness + gap;
            let _x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let _y = offset_y as i32 + (height as i32 - total as i32) / 2;
            let start_x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let start_y = offset_y as i32 + (height as i32 - total as i32) / 2;
            // Horizontal line
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
            // Vertical line
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╗ U+2557: BOX DRAWINGS DOUBLE DOWN AND LEFT
        0x2557 => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total = 2 * thickness + gap;
            let start_x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let start_y = offset_y as i32 + (height as i32 - total as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╚ U+255A: BOX DRAWINGS DOUBLE UP AND RIGHT
        0x255A => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total = 2 * thickness + gap;
            let start_x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let start_y = offset_y as i32 + (height as i32 - total as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╝ U+255D: BOX DRAWINGS DOUBLE UP AND LEFT
        0x255D => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total = 2 * thickness + gap;
            let start_x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let start_y = offset_y as i32 + (height as i32 - total as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╠ U+2560: BOX DRAWINGS DOUBLE VERTICAL AND RIGHT
        0x2560 => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total = 2 * thickness + gap;
            let start_x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let start_y = offset_y as i32 + (height as i32 - total as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╣ U+2563: BOX DRAWINGS DOUBLE VERTICAL AND LEFT
        0x2563 => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total = 2 * thickness + gap;
            let start_x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let start_y = offset_y as i32 + (height as i32 - total as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╦ U+2566: BOX DRAWINGS DOUBLE DOWN AND HORIZONTAL
        0x2566 => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total = 2 * thickness + gap;
            let start_x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let start_y = offset_y as i32 + (height as i32 - total as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╩ U+2569: BOX DRAWINGS DOUBLE UP AND HORIZONTAL
        0x2569 => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total = 2 * thickness + gap;
            let start_x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let start_y = offset_y as i32 + (height as i32 - total as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╬ U+256C: BOX DRAWINGS DOUBLE VERTICAL AND HORIZONTAL
        0x256C => {
            let thickness = line_thickness.max(1);
            let gap = line_thickness.max(1);
            let total = 2 * thickness + gap;
            let start_x = offset_x as i32 + (width as i32 - total as i32) / 2;
            let start_y = offset_y as i32 + (height as i32 - total as i32) / 2;
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y, width, thickness);
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, start_y + thickness as i32 + gap as i32, width, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x, offset_y as i32, height, thickness);
            draw_vertical_line(atlas_data, atlas_width, start_x + thickness as i32 + gap as i32, offset_y as i32, height, thickness);
        }
        // ╱ U+2571: DIAGONAL FALLING
        0x2571 => {
            let t = std::cmp::max(1, line_thickness / 2);
            for i in 0..height.min(width) {
                let px = offset_x + (i * width) / height;
                draw_rect(atlas_data, atlas_width, px as i32, (offset_y + i) as i32, t, 1, Shade::On);
            }
        }
        // ╲ U+2572: DIAGONAL RISING
        0x2572 => {
            let t = std::cmp::max(1, line_thickness / 2);
            for i in 0..height.min(width) {
                let px = offset_x + width - (i * width) / height - 1;
                draw_rect(atlas_data, atlas_width, px as i32, (offset_y + i) as i32, t, 1, Shade::On);
            }
        }
        // ╳ U+2573: DIAGONAL CROSS
        0x2573 => {
            let t = std::cmp::max(1, line_thickness / 2);
            for i in 0..height.min(width) {
                let px1 = offset_x + (i * width) / height;
                let px2 = offset_x + width - (i * width) / height - 1;
                draw_rect(atlas_data, atlas_width, px1 as i32, (offset_y + i) as i32, t, 1, Shade::On);
                draw_rect(atlas_data, atlas_width, px2 as i32, (offset_y + i) as i32, t, 1, Shade::On);
            }
        }
        // 对于其他框绘制字符，使用参数化边缘绘制
        _ => {
            // 每个框绘制字符最多有 4 条边缘（上、右、下、左）
            // 权重为无(0)、细(1) 或粗(2)。
            // 我们从码点解码边缘并相应绘制。
            if let Some(edges) = box_drawing_edges(code) {
                let mid_x = offset_x as i32 + (width as i32 - line_thickness as i32) / 2;
                let mid_y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;

                // 上边缘（从上到中心的垂直线）
                if edges.top > 0 {
                    let t = if edges.top == 2 { line_thickness * 2 } else { line_thickness };
                    let x = offset_x as i32 + (width as i32 - t as i32) / 2;
                    draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height / 2, t);
                }
                // 下边缘（从中心到底部的垂直线）
                if edges.bottom > 0 {
                    let t = if edges.bottom == 2 { line_thickness * 2 } else { line_thickness };
                    let x = offset_x as i32 + (width as i32 - t as i32) / 2;
                    draw_vertical_line(atlas_data, atlas_width, x, mid_y, height / 2, t);
                }
                // 左边缘（从左到中心的水平线）
                if edges.left > 0 {
                    let t = if edges.left == 2 { line_thickness * 2 } else { line_thickness };
                    let y = offset_y as i32 + (height as i32 - t as i32) / 2;
                    draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width / 2, t);
                }
                // 右边缘（从中心到右的水平线）
                if edges.right > 0 {
                    let t = if edges.right == 2 { line_thickness * 2 } else { line_thickness };
                    let y = offset_y as i32 + (height as i32 - t as i32) / 2;
                    draw_horizontal_line(atlas_data, atlas_width, mid_x, y, width / 2, t);
                }
            } else {
                // 回退：绘制细水平线
                let y = offset_y as i32 + (height as i32 - line_thickness as i32) / 2;
                draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, y, width, line_thickness);
            }
        }
    }

    // 推进光标
    *cursor_x += width + 1;

    Some(GlyphInfo {
        x: offset_x,
        y: offset_y,
        width: width as u16,
        height: height as u16,
        x_offset: 0.0,
        y_offset: 0.0,
        advance: width as f32,
        is_ligature: false,
        ligature_len: 1,
    })
}

/// 绘制单个盲文点。
#[inline]
fn draw_braille_dot(
    atlas_data: &mut [u8],
    atlas_width: u32,
    atlas_height: u32,
    offset_x: u32,
    offset_y: u32,
    cell_width: u32,
    cell_height: u32,
    dot_col: u32,
    dot_row: u32,
    dot_radius: u32,
) {
    // 盲文点排列为 2 列 x 3 行
    // 点位置从单元格边缘偏移
    let dot_spacing_x = cell_width / 4;
    let dot_spacing_y = cell_height / 6;

    let cx = offset_x + dot_spacing_x + (dot_col * dot_spacing_x * 2);
    let cy = offset_y + dot_spacing_y + (dot_row * dot_spacing_y * 2);

    // 绘制一个小的填充圆
    for py in 0..dot_radius * 2 {
        for px in 0..dot_radius * 2 {
            let dx = px as i32 - dot_radius as i32;
            let dy = py as i32 - dot_radius as i32;
            if dx * dx + dy * dy <= (dot_radius as i32) * (dot_radius as i32) {
                let ax = cx + px - dot_radius;
                let ay = cy + py - dot_radius;
                if ax < atlas_width && ay < atlas_height {
                    let idx = (ay as usize) * atlas_width as usize + ax as usize;
                    if idx < atlas_data.len() {
                        atlas_data[idx] = Shade::On as u8;
                    }
                }
            }
        }
    }
}

/// 将盲文模式字符绘制到图集数据中。
/// 如果字符被绘制则返回 Some(GlyphInfo)，如果不支持则返回 None。
pub fn draw_braille(
    atlas_data: &mut [u8],
    atlas_width: u32,
    atlas_height: u32,
    metrics: &SpriteMetrics,
    c: char,
    cursor_x: &mut u32,
    cursor_y: &mut u32,
) -> Option<GlyphInfo> {
    let code = c as u32;
    let width = metrics.cell_width;
    let height = metrics.cell_height;
    let offset_x = *cursor_x;
    let offset_y = *cursor_y;

    // 盲文基础是 U+2800
    let pattern = code - 0x2800;

    // 点位置：每个位代表一个点 (0-7)
    // 0  3
    // 1  4
    // 2  5
    // 6  7（很少使用）
    let dot_radius = std::cmp::max(1, width.min(height) / 8);

    // 根据模式绘制点
    // 位 0 -> 点 (0,0), 位 1 -> 点 (0,1), 位 2 -> 点 (0,2)
    // 位 3 -> 点 (1,0), 位 4 -> 点 (1,1), 位 5 -> 点 (1,2)
    // 位 6 -> 点 (0,3), 位 7 -> 点 (1,3)
    if pattern & 0x001 != 0 {
        draw_braille_dot(atlas_data, atlas_width, atlas_height, offset_x, offset_y, width, height, 0, 0, dot_radius);
    }
    if pattern & 0x002 != 0 {
        draw_braille_dot(atlas_data, atlas_width, atlas_height, offset_x, offset_y, width, height, 0, 1, dot_radius);
    }
    if pattern & 0x004 != 0 {
        draw_braille_dot(atlas_data, atlas_width, atlas_height, offset_x, offset_y, width, height, 0, 2, dot_radius);
    }
    if pattern & 0x008 != 0 {
        draw_braille_dot(atlas_data, atlas_width, atlas_height, offset_x, offset_y, width, height, 1, 0, dot_radius);
    }
    if pattern & 0x010 != 0 {
        draw_braille_dot(atlas_data, atlas_width, atlas_height, offset_x, offset_y, width, height, 1, 1, dot_radius);
    }
    if pattern & 0x020 != 0 {
        draw_braille_dot(atlas_data, atlas_width, atlas_height, offset_x, offset_y, width, height, 1, 2, dot_radius);
    }
    if pattern & 0x040 != 0 {
        draw_braille_dot(atlas_data, atlas_width, atlas_height, offset_x, offset_y, width, height, 0, 3, dot_radius);
    }
    if pattern & 0x080 != 0 {
        draw_braille_dot(atlas_data, atlas_width, atlas_height, offset_x, offset_y, width, height, 1, 3, dot_radius);
    }

    // 推进光标
    *cursor_x += width + 1;

    Some(GlyphInfo {
        x: offset_x,
        y: offset_y,
        width: width as u16,
        height: height as u16,
        x_offset: 0.0,
        y_offset: 0.0,
        advance: width as f32,
        is_ligature: false,
        ligature_len: 1,
    })
}

/// 将 Powerline 符号绘制到图集数据中。
/// 如果字符被绘制则返回 Some(GlyphInfo)，如果不支持则返回 None。
pub fn draw_powerline(
    atlas_data: &mut [u8],
    atlas_width: u32,
    atlas_height: u32,
    metrics: &SpriteMetrics,
    c: char,
    cursor_x: &mut u32,
    cursor_y: &mut u32,
) -> Option<GlyphInfo> {
    let code = c as u32;
    let width = metrics.cell_width;
    let height = metrics.cell_height;
    let offset_x = *cursor_x;
    let offset_y = *cursor_y;

    // Powerline 符号是三角形分隔符
    // 常见的有：               
    // 它们在私人使用区，从 U+E0B0 偏移

    match code {
        //  U+E0B0: 右箭头
        0xE0B0 => {
            // 绘制一个简单的右指三角形
            let tri_height = height / 2;
            let tri_width = width / 2;
            let _start_x = offset_x;
            let start_y = offset_y + (height - tri_height) / 2;

            for py in 0..tri_height {
                // 每一行的像素逐渐增加
                let row_width = ((py + 1) * tri_width) / tri_height;
                let row_start_x = offset_x + width - row_width;
                draw_rect(atlas_data, atlas_width, row_start_x as i32, (start_y + py) as i32, row_width, 1, Shade::On);
            }
        }
        //  U+E0B1: 左箭头（反向）
        0xE0B1 => {
            let tri_height = height / 2;
            let tri_width = width / 2;
            let start_y = offset_y + (height - tri_height) / 2;

            for py in 0..tri_height {
                let row_width = ((py + 1) * tri_width) / tri_height;
                draw_rect(atlas_data, atlas_width, offset_x as i32, (start_y + py) as i32, row_width, 1, Shade::On);
            }
        }
        //  U+E0B2: 细右箭头
        0xE0B2 => {
            // 更细的三角形
            let tri_height = height / 3;
            let tri_width = width / 3;
            let start_y = offset_y + (height - tri_height) / 2;

            for py in 0..tri_height {
                let row_width = ((py + 1) * tri_width) / tri_height;
                let row_start_x = offset_x + width - row_width;
                draw_rect(atlas_data, atlas_width, row_start_x as i32, (start_y + py) as i32, row_width, 1, Shade::On);
            }
        }
        //  U+E0B4: 火焰（左下到右上）
        0xE0B4 => {
            // 绘制对角条纹
            for py in 0..height {
                let stripe_x = offset_x + ((py * width) / height);
                draw_rect(atlas_data, atlas_width, stripe_x as i32, (offset_y + py) as i32, 1, 1, Shade::On);
            }
        }
        //  U+E0B5: FLAME (top-left to bottom-right)
        0xE0B5 => {
            for py in 0..height {
                let stripe_x = offset_x + width - ((py * width) / height) - 1;
                if stripe_x >= offset_x {
                    draw_rect(atlas_data, atlas_width, stripe_x as i32, (offset_y + py) as i32, 1, 1, Shade::On);
                }
            }
        }
        //  U+E0B6: 凹入模式行箭头
        0xE0B6 => {
            // 绘制左下到右上的对角线
            let thickness = std::cmp::max(1, width.min(height) / 10);
            for py in 0..height {
                let stripe_x = offset_x + ((py * width) / height);
                draw_rect(atlas_data, atlas_width, stripe_x as i32, (offset_y + py) as i32, thickness, 1, Shade::On);
            }
        }
        //  U+E0B7: 模式行不完整标志
        0xE0B7 => {
            // 类似于  但更短
            let thickness = std::cmp::max(1, width.min(height) / 10);
            let half_h = height / 2;
            for py in 0..half_h {
                let stripe_x = offset_x + ((py * width) / half_h);
                draw_rect(atlas_data, atlas_width, stripe_x as i32, (offset_y + py) as i32, thickness, 1, Shade::On);
            }
        }
        //  U+E0B8: 模式行左箭头
        0xE0B8 => {
            // 右下到左上的对角线
            let thickness = std::cmp::max(1, width.min(height) / 10);
            for py in 0..height {
                let stripe_x = offset_x + width - ((py * width) / height) - thickness;
                if stripe_x >= offset_x {
                    draw_rect(atlas_data, atlas_width, stripe_x as i32, (offset_y + py) as i32, thickness, 1, Shade::On);
                }
            }
        }
        //  U+E0B9: 模式行波浪箭头
        0xE0B9 => {
            // Draw a wavy pattern
            let thickness = std::cmp::max(1, width.min(height) / 10);
            for px in 0..width {
                let wave = ((px * 4 * 3) / width) % 4;
                let wave_y = if wave < 2 { wave } else { 4 - wave };
                let py = height / 4 + wave_y * (height / 8);
                draw_rect(atlas_data, atlas_width, (offset_x + px) as i32, (offset_y + py) as i32, 1, thickness, Shade::On);
            }
        }
        //  U+E0BA: 模式行波浪右箭头
        0xE0BA => {
            let thickness = std::cmp::max(1, width.min(height) / 10);
            for px in 0..width {
                let wave = ((px * 4 * 3) / width) % 4;
                let wave_y = if wave < 2 { wave } else { 4 - wave };
                let py = height / 4 + wave_y * (height / 8);
                draw_rect(atlas_data, atlas_width, (offset_x + px) as i32, (offset_y + py) as i32, 1, thickness, Shade::On);
            }
        }
        //  U+E0BC: 分支 git 分支
        0xE0BC => {
            // 绘制带一条线的圆（简化的分支符号）
            let radius = width.min(height) / 3;
            let cx = offset_x + width / 2;
            let cy = offset_y + height / 2;

            // 绘制圆轮廓
            for angle in 0..360 {
                let rad = (angle as f32) * std::f32::consts::PI / 180.0;
                let px = cx as i32 + (rad.cos() * radius as f32) as i32;
                let py = cy as i32 + (rad.sin() * radius as f32) as i32;
                if px >= 0 && py >= 0 && (px as u32) < atlas_width && (py as u32) < atlas_height {
                    let idx = (py as usize) * atlas_width as usize + px as usize;
                    if idx < atlas_data.len() {
                        atlas_data[idx] = Shade::On as u8;
                    }
                }
            }

            // 在底部绘制垂直线
            let thickness = std::cmp::max(1, width.min(height) / 10);
            draw_vertical_line(atlas_data, atlas_width, cx as i32 - thickness as i32 / 2, cy as i32, radius, thickness);
        }
        //  U+E0BD: GIT 已修改
        0xE0BD => {
            // 绘制一个简单的 '>' 形状
            let tri_height = height / 2;
            let tri_width = width / 2;
            let start_y = offset_y + (height - tri_height) / 2;

            for py in 0..tri_height {
                let row_width = ((tri_height - py) * tri_width) / tri_height;
                let row_start_x = offset_x + width / 2 - row_width / 2;
                draw_rect(atlas_data, atlas_width, row_start_x as i32, (start_y + py) as i32, row_width, 1, Shade::On);
            }
        }
        //  U+E0BE: GIT 已暂存
        0xE0BE => {
            // 绘制一个指向左边的倒三角形
            let tri_height = height / 2;
            let tri_width = width / 2;
            let start_y = offset_y + (height - tri_height) / 2;

            for py in 0..tri_height {
                let row_width = ((py + 1) * tri_width) / tri_height;
                draw_rect(atlas_data, atlas_width, offset_x as i32, (start_y + py) as i32, row_width, 1, Shade::On);
            }
        }
        //  U+E0BF: GIT 未跟踪
        0xE0BF => {
            // 绘制一个星形（简化版）
            let thickness = std::cmp::max(1, width.min(height) / 12);
            // 垂直线
            draw_vertical_line(atlas_data, atlas_width, (offset_x + width / 2) as i32 - thickness as i32 / 2, offset_y as i32, height, thickness);
            // 水平线
            draw_horizontal_line(atlas_data, atlas_width, offset_x as i32, (offset_y + height / 2) as i32, width, thickness);
            // 对角线
            for i in 0..height {
                let offset = (i * width) / height;
                draw_rect(atlas_data, atlas_width, (offset_x + offset) as i32, (offset_y + i) as i32, thickness, 1, Shade::On);
                draw_rect(atlas_data, atlas_width, (offset_x + width - offset - thickness as u32) as i32, (offset_y + i) as i32, thickness, 1, Shade::On);
            }
        }
        // 对于未知的 powerline 字符，绘制一个简单的垂直线
        _ => {
            let thickness = std::cmp::max(1, width.min(height) / 6);
            let x = offset_x as i32 + (width as i32 - thickness as i32) / 2;
            draw_vertical_line(atlas_data, atlas_width, x, offset_y as i32, height, thickness);
        }
    }

    // 推进光标
    *cursor_x += width + 1;

    Some(GlyphInfo {
        x: offset_x,
        y: offset_y,
        width: width as u16,
        height: height as u16,
        x_offset: 0.0,
        y_offset: 0.0,
        advance: width as f32,
        is_ligature: false,
        ligature_len: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_block_element() {
        assert!(is_block_element('█'));
        assert!(is_block_element('▐'));
        assert!(is_block_element('▛'));
        assert!(!is_block_element('A'));
        assert!(!is_block_element(' '));
    }

    #[test]
    fn test_draw_full_block() {
        let mut atlas_data = vec![0u8; 100 * 100];
        let metrics = SpriteMetrics::new(10, 20, 14.0);
        let mut cursor_x = 0u32;
        let mut cursor_y = 0u32;

        let result = draw_block_element(&mut atlas_data, 100, &metrics, '█', &mut cursor_x, &mut cursor_y);

        assert!(result.is_some());
        // Check that some pixels are filled
        let filled = atlas_data.iter().filter(|&&x| x != 0).count();
        assert!(filled > 0, "Full block should fill the cell");

        // Verify full block fills entire cell (10x20 = 200 pixels)
        let info = result.unwrap();
        assert_eq!(info.width, 10);
        assert_eq!(info.height, 20);
        assert_eq!(filled, 200, "Full block should fill all {}x{}={} pixels", info.width, info.height, info.width as usize * info.height as usize);

        // Check first pixel is filled
        assert_eq!(atlas_data[0], 0xFF, "First pixel should be fully opaque");
    }

    #[test]
    fn test_is_box_drawing() {
        assert!(is_box_drawing('─'));
        assert!(is_box_drawing('│'));
        assert!(is_box_drawing('┌'));
        assert!(is_box_drawing('┼'));
        assert!(!is_box_drawing('A'));
        assert!(!is_box_drawing(' '));
    }

    #[test]
    fn test_is_braille() {
        assert!(is_braille('⠀'));
        assert!(is_braille('⠁'));
        assert!(is_braille('⣿'));
        assert!(!is_braille('A'));
        assert!(!is_braille(' '));
    }

    #[test]
    fn test_is_powerline() {
        assert!(is_powerline('\u{E0B0}'));
        assert!(is_powerline('\u{E0B4}'));
        assert!(!is_powerline('A'));
        assert!(!is_powerline(' '));
    }

    #[test]
    fn test_draw_box_drawing() {
        let mut atlas_data = vec![0u8; 100 * 100];
        let metrics = SpriteMetrics::new(10, 20, 14.0);
        let mut cursor_x = 0u32;
        let mut cursor_y = 0u32;

        // Test horizontal line
        let result = draw_box_drawing(&mut atlas_data, 100, &metrics, '─', &mut cursor_x, &mut cursor_y);
        assert!(result.is_some());
        let filled = atlas_data.iter().filter(|&&x| x != 0).count();
        assert!(filled > 0, "Box drawing should fill some pixels");

        // Test vertical line
        cursor_x = 0;
        cursor_y = 0;
        let result2 = draw_box_drawing(&mut atlas_data, 100, &metrics, '│', &mut cursor_x, &mut cursor_y);
        assert!(result2.is_some());
    }

    #[test]
    fn test_draw_braille() {
        let mut atlas_data = vec![0u8; 100 * 100];
        let metrics = SpriteMetrics::new(10, 20, 14.0);
        let mut cursor_x = 0u32;
        let mut cursor_y = 0u32;

        // Test braille pattern (all dots)
        let result = draw_braille(&mut atlas_data, 100, 100, &metrics, '⣿', &mut cursor_x, &mut cursor_y);
        assert!(result.is_some());
        let filled = atlas_data.iter().filter(|&&x| x != 0).count();
        assert!(filled > 0, "Braille should fill some pixels");
    }

    #[test]
    fn test_draw_powerline() {
        let mut atlas_data = vec![0u8; 100 * 100];
        let metrics = SpriteMetrics::new(10, 20, 14.0);
        let mut cursor_x = 0u32;
        let mut cursor_y = 0u32;

        // Test powerline arrow
        let result = draw_powerline(&mut atlas_data, 100, 100, &metrics, '\u{E0B0}', &mut cursor_x, &mut cursor_y);
        assert!(result.is_some());
        let filled = atlas_data.iter().filter(|&&x| x != 0).count();
        assert!(filled > 0, "Powerline should fill some pixels");
    }
}
