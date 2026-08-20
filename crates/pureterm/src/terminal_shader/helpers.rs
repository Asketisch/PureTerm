//! 渲染辅助函数:小工具函数(坐标位转换、背景四边形构造、颜色转换)。
//!
//! 这里集中放置 `Primitive::prepare/draw` 多处复用的小工具,避免散落在
//! 各处 impl 块中。逻辑与拆分前完全一致,只做物理搬运。

use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

use super::vertices::BgVertex;

/// 共享缩放因子:由 `prepare()`(渲染线程)写入、
/// `draw()`(UI 线程)读取。以 `AtomicU32` 中的 `f32` 位存储。
pub(super) fn f32_to_bits(f: f32) -> u32 { f.to_bits() }
pub(super) fn bits_to_f32(u: u32) -> f32 { f32::from_bits(u) }

/// 把一块屏幕空间背景四边形(4 个顶点 + 6 个索引)追加到给定的
/// 顶点/索引切片。`TerminalProgram::draw()` 用它填充单元格
/// 背景、选区/光标覆盖层等。
pub(in crate::terminal_shader) fn push_bg_quad(
    v: &mut Vec<BgVertex>,
    i: &mut Vec<u32>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
) {
    let base = v.len() as u32;
    v.extend([
        BgVertex { position: [x, y], color },
        BgVertex { position: [x + w, y], color },
        BgVertex { position: [x + w, y + h], color },
        BgVertex { position: [x, y + h], color },
    ]);
    i.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// 把 alacritty 的 `Color`(Named/Spec/Indexed)解析为归一化的
/// `[f32; 4]` RGBA(范围 `0.0..=1.0`),Named/Indexed 条目
/// 从活动 `Colors` 数组查表。
pub(in crate::terminal_shader) fn color_to_f32(
    color: Color,
    colors: &alacritty_terminal::term::color::Colors,
) -> [f32; 4] {
    match color {
        Color::Named(named) => {
            let rgb = colors[named].unwrap_or(named_color_default(named));
            rgb_to_f32(rgb)
        }
        Color::Spec(rgb) => rgb_to_f32(rgb),
        Color::Indexed(idx) => {
            let rgb = colors[idx as usize].unwrap_or(Rgb { r: 204, g: 204, b: 204 });
            rgb_to_f32(rgb)
        }
    }
}

/// 当活动 `Colors` 数组没有填充对应槽位时(如光标/背景
/// 未在配置里设置),`NamedColor` 变体使用的
/// 硬编码回退 RGB。
pub(in crate::terminal_shader) fn named_color_default(named: NamedColor) -> Rgb {
    match named {
        NamedColor::Black => Rgb { r: 0, g: 0, b: 0 },
        NamedColor::Red => Rgb { r: 204, g: 0, b: 0 },
        NamedColor::Green => Rgb { r: 78, g: 154, b: 6 },
        NamedColor::Yellow => Rgb { r: 196, g: 160, b: 0 },
        NamedColor::Blue => Rgb { r: 52, g: 101, b: 164 },
        NamedColor::Magenta => Rgb { r: 117, g: 80, b: 123 },
        NamedColor::Cyan => Rgb { r: 6, g: 152, b: 154 },
        NamedColor::White => Rgb { r: 211, g: 215, b: 207 },
        NamedColor::BrightBlack => Rgb { r: 85, g: 87, b: 83 },
        NamedColor::BrightRed => Rgb { r: 239, g: 41, b: 41 },
        NamedColor::BrightGreen => Rgb { r: 138, g: 226, b: 52 },
        NamedColor::BrightYellow => Rgb { r: 252, g: 233, b: 79 },
        NamedColor::BrightBlue => Rgb { r: 114, g: 159, b: 207 },
        NamedColor::BrightMagenta => Rgb { r: 173, g: 127, b: 168 },
        NamedColor::BrightCyan => Rgb { r: 52, g: 226, b: 226 },
        NamedColor::BrightWhite => Rgb { r: 238, g: 238, b: 236 },
        NamedColor::Foreground => Rgb { r: 211, g: 215, b: 207 },
        NamedColor::Background => Rgb { r: 28, g: 28, b: 30 },
        NamedColor::Cursor => Rgb { r: 211, g: 215, b: 207 },
        _ => Rgb { r: 204, g: 204, b: 204 },
    }
}

pub(in crate::terminal_shader) fn rgb_to_f32(rgb: Rgb) -> [f32; 4] {
    [rgb.r as f32 / 255.0, rgb.g as f32 / 255.0, rgb.b as f32 / 255.0, 1.0]
}
