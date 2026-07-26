//! 渲染辅助函数:小工具函数(坐标位转换、背景四边形构造、颜色转换)。
//!
//! 这里集中放置 `Primitive::prepare/draw` 多处复用的小工具,避免散落在
//! 各处 impl 块中。逻辑与拆分前完全一致,只做物理搬运。

use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

use super::vertices::BgVertex;

/// Shared scale factor written by `prepare()` (renderer threads) and read by
/// `draw()` (UI thread). Stored as `f32` bits in an `AtomicU32`.
pub(super) fn f32_to_bits(f: f32) -> u32 { f.to_bits() }
pub(super) fn bits_to_f32(u: u32) -> f32 { f32::from_bits(u) }

/// Append a screen-space bg quad (4 vertices + 6 indices) to the given
/// vertex/index slices. Used by `TerminalProgram::draw()` to fill cell
/// backgrounds, selection/cursor overlays, etc.
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

/// Resolve an alacritty `Color` (Named/Spec/Indexed) into a normalized
/// `[f32; 4]` RGBA in `0.0..=1.0`, looking up the active `Colors` array
/// for Named/Indexed entries.
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

/// Hard-coded fallback RGB for `NamedColor` variants when the active
/// `Colors` array has no slot filled (e.g. cursor / background not set
/// via config).
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
