//! GAP-72：终端配色覆盖 —— 预设把 ANSI 16 色与叠加层色注入渲染管线。
//!
//! 关键点(见 spec §2.3):终端内核的 `Colors` 是一条 269 槽扁平数组,
//! `NamedColor::Black..BrightWhite` 的判别值正好是 `0..15`,与 256 色
//! 调色板的前 16 项**同槽**。因此往这 16 个槽位各写一个颜色,即可同时
//! 覆盖「具名色」(SGR 31/42 等)与「索引色」(SGR `38;5;N`)——
//! 不碰 vendored 内核、不新增 GPU 通道。
//!
//! 每帧成本:16 次 `Option<Rgb>` 写入 + 几个常量替换,相对每帧数十万
//! cell 的遍历可忽略。

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{NamedColor, Rgb};

/// ANSI 16 色对应的 `NamedColor` 槽位。
///
/// 顺序必须与 `NamedColor` 判别值一致(0..15),这样写入 256 色调色板
/// 的前 16 项时不会错位。
pub const ANSI_SLOTS: [NamedColor; 16] = [
    NamedColor::Black,
    NamedColor::Red,
    NamedColor::Green,
    NamedColor::Yellow,
    NamedColor::Blue,
    NamedColor::Magenta,
    NamedColor::Cyan,
    NamedColor::White,
    NamedColor::BrightBlack,
    NamedColor::BrightRed,
    NamedColor::BrightGreen,
    NamedColor::BrightYellow,
    NamedColor::BrightBlue,
    NamedColor::BrightMagenta,
    NamedColor::BrightCyan,
    NamedColor::BrightWhite,
];

/// 终端内叠加层的配色(光标 / 选区 / 搜索命中 / 折叠遮罩)。
///
/// 字段是归一化的 RGBA(`0.0..=1.0`),与渲染管线内部一致。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TermColorOverrides {
    /// 光标色。
    pub cursor: [f32; 4],
    /// 选区高亮色。
    pub selection: [f32; 4],
    /// 搜索命中高亮色。
    pub search_hit: [f32; 4],
    /// 触发器折叠区的遮罩色。
    pub fold_conceal: [f32; 4],
    /// vi 模式光标色。
    pub vi_cursor: [f32; 4],
    /// vi 模式下被压暗的主光标色。
    pub vi_dim: [f32; 4],
}

impl Default for TermColorOverrides {
    /// 与引入 GAP-72 之前 `program.rs` 里的硬编码常量**逐值一致** ——
    /// classic 预设因此保持逐像素不变。
    fn default() -> Self {
        Self {
            cursor: [0.6, 0.6, 0.6, 0.8],
            selection: [0.30, 0.50, 0.90, 0.40],
            search_hit: [0.85, 0.65, 0.15, 0.35],
            fold_conceal: [0.15, 0.15, 0.18, 0.85],
            vi_cursor: [0.2, 0.8, 0.7, 0.85],
            vi_dim: [0.4, 0.4, 0.4, 0.3],
        }
    }
}

/// 把预设的 ANSI 16 色写入 `colors` 数组。
///
/// 覆盖具名色与 `38;5;N` 索引色两条路径(两者同槽)。
pub fn inject_ansi_palette(colors: &mut Colors, ansi: &[Rgb; 16]) {
    for (slot, rgb) in ANSI_SLOTS.iter().zip(ansi.iter()) {
        colors[*slot as usize] = Some(*rgb);
    }
}

/// 归一化 RGBA → `Rgb`(0..255 整数通道)。
pub fn rgb_from_rgba(rgba: [f32; 4]) -> Rgb {
    Rgb {
        r: (rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        g: (rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        b: (rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_slots_are_contiguous_from_zero() {
        // 槽位必须是 0..15 —— 与 256 色调色板前 16 项同槽的前提。
        for (i, slot) in ANSI_SLOTS.iter().enumerate() {
            assert_eq!(*slot as usize, i, "ANSI 槽位 {i} 错位");
        }
    }

    #[test]
    fn inject_ansi_writes_both_named_and_indexed_slots() {
        let mut colors = Colors::default();
        let ansi: [Rgb; 16] = std::array::from_fn(|i| Rgb { r: i as u8, g: 0, b: 0 });
        inject_ansi_palette(&mut colors, &ansi);

        // 具名色路径。
        assert_eq!(colors[NamedColor::Red as usize], Some(Rgb { r: 1, g: 0, b: 0 }));
        assert_eq!(
            colors[NamedColor::BrightWhite as usize],
            Some(Rgb { r: 15, g: 0, b: 0 })
        );
        // 索引色路径(`38;5;N`)—— 与具名色同槽。
        assert_eq!(colors[1], Some(Rgb { r: 1, g: 0, b: 0 }));
        assert_eq!(colors[15], Some(Rgb { r: 15, g: 0, b: 0 }));
        // 未覆盖的槽位保持为空(回落到内核默认)。
        assert_eq!(colors[200], None);
    }

    #[test]
    fn rgb_conversion_clamps_and_rounds() {
        assert_eq!(rgb_from_rgba([1.0, 0.0, 0.0, 1.0]), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(rgb_from_rgba([0.5, 0.5, 0.5, 1.0]), Rgb { r: 128, g: 128, b: 128 });
        // 越界值被夹紧,不回绕。
        assert_eq!(rgb_from_rgba([2.0, -1.0, 0.0, 1.0]), Rgb { r: 255, g: 0, b: 0 });
    }

    #[test]
    fn default_overrides_match_legacy_constants() {
        let o = TermColorOverrides::default();
        assert_eq!(o.cursor, [0.6, 0.6, 0.6, 0.8]);
        assert_eq!(o.selection, [0.30, 0.50, 0.90, 0.40]);
        assert_eq!(o.search_hit, [0.85, 0.65, 0.15, 0.35]);
        assert_eq!(o.fold_conceal, [0.15, 0.15, 0.18, 0.85]);
        assert_eq!(o.vi_cursor, [0.2, 0.8, 0.7, 0.85]);
        assert_eq!(o.vi_dim, [0.4, 0.4, 0.4, 0.3]);
    }
}
