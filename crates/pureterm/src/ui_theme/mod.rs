//! GAP-72：主应用的**界面主题层**。
//!
//! 调色板与预设的**唯一真源**在 `pureterm-settings` crate
//! (`pureterm_settings::theme::palette`)—— 因为主应用与设置窗口是两个
//! 进程,必须共用同一份色值定义,反向依赖会成环。
//!
//! 本模块负责:
//! - 把调色板类型再导出,方便主 crate 内部统一引用;
//! - 提供外壳样式助手([`shell_style`]),把散落在各 `view_*` 里的内联
//!   `container::Style` / `Border` / `Shadow` 收敛成可复用函数。

pub mod shell_style;

pub use pureterm_settings::theme::palette::{ShellTokens, ThemePreset, UiPalette};

use alacritty_terminal::vte::ansi::Rgb;

use crate::terminal_shader::{rgb_from_rgba, TermColorOverrides};

/// 预设的 ANSI 16 色 → 终端内核的 `Rgb`(0..255 通道)。
///
/// 注入 `Colors` 的 0..15 槽位后,具名色与 `38;5;N` 索引色同时生效。
pub fn ansi_rgb(palette: UiPalette) -> [Rgb; 16] {
    palette
        .terminal
        .ansi
        .map(|c| rgb_from_rgba([c.r, c.g, c.b, c.a]))
}

/// 预设的终端叠加层配色(光标 / 选区 / 搜索命中 / 折叠遮罩 / vi 光标)。
pub fn term_overrides(palette: UiPalette) -> TermColorOverrides {
    let t = palette.terminal;
    let rgba = |c: iced::Color| [c.r, c.g, c.b, c.a];
    TermColorOverrides {
        cursor: rgba(t.cursor),
        selection: rgba(t.selection),
        search_hit: rgba(t.search_hit),
        fold_conceal: rgba(t.fold_conceal),
        vi_cursor: rgba(t.vi_cursor),
        vi_dim: rgba(t.vi_dim),
    }
}

/// 终端渲染的配色接线结果 —— 启动时算一次,逐帧只读。
#[derive(Clone, Copy, Debug)]
pub struct TerminalWiring {
    /// ANSI 16 色覆盖。`None` = 用内核默认色板(今天的观感)。
    pub ansi: Option<[Rgb; 16]>,
    /// 终端内叠加层配色(光标 / 选区 / 搜索命中 / 折叠遮罩 / vi 光标)。
    pub overrides: TermColorOverrides,
    /// 终端背景色(RGBA,0..=1)。
    pub background: [f32; 4],
    /// 终端前景色(RGBA,0..=1)。
    pub foreground: [f32; 4],
}

/// 依据预设与 `[appearance].terminal_palette` 决定终端配色。
///
/// 规则(见 GAP-72 spec §4.2):
/// - `classic` → **完全走用户配置**(`[colors.primary]`),不注入 ANSI 色
///   —— 与引入本特性之前逐像素一致;
/// - `cyberpunk` + `terminal_palette = "follow"`(默认)→ 终端前景/背景/
///   ANSI 16 色/叠加层全部取预设;
/// - `cyberpunk` + `terminal_palette = "user"` → 只换外壳,终端保留
///   用户自己的 `[colors.*]`(逃生阀)。
pub fn terminal_wiring(
    palette: UiPalette,
    preset: ThemePreset,
    keep_user_terminal_colors: bool,
    user_bg: [f32; 4],
    user_fg: [f32; 4],
) -> TerminalWiring {
    let t = palette.terminal;
    let preset_wins = preset == ThemePreset::Cyberpunk && !keep_user_terminal_colors;
    let (ansi, background, foreground) = if preset_wins {
        (
            Some(ansi_rgb(palette)),
            [t.bg.r, t.bg.g, t.bg.b, t.bg.a],
            [t.fg.r, t.fg.g, t.fg.b, t.fg.a],
        )
    } else {
        (None, user_bg, user_fg)
    };
    TerminalWiring {
        ansi,
        overrides: term_overrides(palette),
        background,
        foreground,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cyber_palette() -> UiPalette {
        UiPalette::for_preset(ThemePreset::Cyberpunk)
    }

    #[test]
    fn classic_keeps_user_terminal_colors() {
        let p = UiPalette::for_preset(ThemePreset::Classic);
        let w = terminal_wiring(p, ThemePreset::Classic, false, [0.1, 0.2, 0.3, 1.0], [0.9, 0.8, 0.7, 1.0]);
        assert!(w.ansi.is_none(), "classic 不得注入 ANSI 色");
        assert_eq!(w.background, [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(w.foreground, [0.9, 0.8, 0.7, 1.0]);
    }

    #[test]
    fn cyberpunk_overrides_terminal_colors() {
        let p = cyber_palette();
        let w = terminal_wiring(p, ThemePreset::Cyberpunk, false, [0.1, 0.2, 0.3, 1.0], [0.9, 0.8, 0.7, 1.0]);
        let ansi = w.ansi.expect("cyberpunk 应注入 ANSI 色");
        assert_eq!(ansi.len(), 16);
        assert_eq!(w.background[0], p.terminal.bg.r);
        assert_eq!(w.foreground[0], p.terminal.fg.r);
    }

    #[test]
    fn cyberpunk_with_user_terminal_palette_keeps_user_colors() {
        let p = cyber_palette();
        let w = terminal_wiring(p, ThemePreset::Cyberpunk, true, [0.1, 0.2, 0.3, 1.0], [0.9, 0.8, 0.7, 1.0]);
        assert!(w.ansi.is_none(), "`terminal_palette=user` 不得注入 ANSI 色");
        assert_eq!(w.background, [0.1, 0.2, 0.3, 1.0]);
    }

    #[test]
    fn classic_overrides_match_legacy_constants() {
        let p = UiPalette::for_preset(ThemePreset::Classic);
        let o = term_overrides(p);
        assert_eq!(o.cursor, [0.6, 0.6, 0.6, 0.8]);
        assert_eq!(o.selection, [0.30, 0.50, 0.90, 0.40]);
    }
}
