//! `palette.rs` 的单元测试。
//!
//! 重点是两条护栏:
//! 1. **classic 预设必须逐值等于今天的效果** —— 任何改动若悄悄改了 classic
//!    的色值,这里会红(默认路径零回归的保证)。
//! 2. **cyberpunk 的色值**必须与 HTML 视觉稿 `docs/preview/cyberpunk-ui.html`
//!    一致(两边都改了会红 —— 防止视觉稿与实现漂移)。

use super::*;
use iced::color;

/// 相对亮度(用于比较亮暗)。
fn luma(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

#[test]
fn preset_parse_exact_match() {
    assert_eq!(ThemePreset::parse("classic"), ThemePreset::Classic);
    assert_eq!(ThemePreset::parse("cyberpunk"), ThemePreset::Cyberpunk);
    assert_eq!(ThemePreset::parse("  cyberpunk  "), ThemePreset::Cyberpunk);
}

#[test]
fn preset_parse_falls_back_to_classic() {
    // 空值 / 未知值 / 大小写不一致 / 别名 → 一律 classic。
    for bad in ["", "  ", "foo", "Cyberpunk", "CYBERPUNK", "cyber", "neon", "赛博朋克"] {
        assert_eq!(
            ThemePreset::parse(bad),
            ThemePreset::Classic,
            "非法 preset {bad:?} 必须回落 classic"
        );
    }
}

#[test]
fn preset_roundtrip_and_options() {
    for preset in ThemePreset::options() {
        assert_eq!(ThemePreset::parse(preset.as_str()), *preset);
        assert!(!preset.label().is_empty());
    }
    assert_eq!(ThemePreset::default(), ThemePreset::Classic);
}

/// classic 的主应用外壳色必须等于**今天**的字面量 —— 这是"默认路径零回归"
/// 的核心护栏。改动任一值都会让本测试红。
#[test]
fn classic_app_tokens_match_legacy_literals() {
    let app = UiPalette::for_preset(ThemePreset::Classic).app;
    assert_eq!(app.bg, Color::from_rgb(0.11, 0.11, 0.12));
    assert_eq!(app.surface, Color::from_rgba(0.10, 0.10, 0.12, 1.0));
    assert_eq!(app.hairline, Color::from_rgba(0.25, 0.25, 0.28, 1.0));
    assert_eq!(app.highlight, Color::from_rgba(1.0, 1.0, 1.0, 0.12));
    assert_eq!(app.highlight_soft, Color::from_rgba(1.0, 1.0, 1.0, 0.06));
    assert_eq!(app.highlight_faint, Color::from_rgba(1.0, 1.0, 1.0, 0.10));
    assert_eq!(app.accent, Color::from_rgba(0.35, 0.75, 0.65, 1.0));
    assert_eq!(app.info, Color::from_rgba(0.3, 0.6, 1.0, 1.0));
    assert_eq!(app.warning, Color::from_rgba(0.95, 0.75, 0.3, 1.0));
    assert_eq!(app.danger, Color::from_rgba(0.9, 0.5, 0.5, 1.0));
    assert_eq!(app.text_primary, Color::WHITE);
    assert_eq!(app.text_secondary, Color::from_rgba(0.55, 0.55, 0.55, 1.0));
    assert_eq!(app.text_dim, Color::from_rgba(1.0, 1.0, 1.0, 0.4));
    assert_eq!(app.text_faint, Color::from_rgba(1.0, 1.0, 1.0, 0.35));
    assert_eq!(app.muted, Color::from_rgba(0.6, 0.6, 0.6, 1.0));
    assert_eq!(app.glow, Color::from_rgba(0.2, 0.8, 0.7, 0.6));
    assert_eq!(app.overlay_bg, Color::from_rgba(0.18, 0.18, 0.20, 0.97));
    assert_eq!(app.overlay_border, Color::from_rgba(1.0, 1.0, 1.0, 0.1));
    assert_eq!(app.shadow, Color::from_rgba(0.0, 0.0, 0.0, 0.5));
    // 顶/底栏底色 = `status_bar::bar_bg(&Theme::Dark)`(`0x2B2D31` × 0.85)。
    assert_eq!(app.elevated.r, 43.0 / 255.0 * 0.85);
    assert_eq!(app.elevated.g, 45.0 / 255.0 * 0.85);
    assert_eq!(app.elevated.b, 49.0 / 255.0 * 0.85);
}

/// classic 的设置窗口色必须等于 `theme/mod.rs` 里原有的常量。
#[test]
fn classic_settings_tokens_match_legacy_constants() {
    let s = UiPalette::for_preset(ThemePreset::Classic).settings;
    assert_eq!(s.bg, color!(0x2b292d));
    assert_eq!(s.surface, color!(0x383539));
    assert_eq!(s.surface_hover, color!(0x44414a));
    assert_eq!(s.hairline, color!(0x504d55));
    assert_eq!(s.accent, color!(0xd3869b));
    assert_eq!(s.accent_hover, color!(0xe09baf));
    assert_eq!(s.success, color!(0x90a959));
    assert_eq!(s.warning, color!(0xf4bf75));
    assert_eq!(s.danger, color!(0xac4242));
    assert_eq!(s.text_primary, color!(0xfecdb2));
    assert_eq!(s.text_secondary, color!(0xb0a392));
}

/// classic 的终端色必须等于今天实际渲染出来的值。
#[test]
fn classic_terminal_tokens_match_legacy_literals() {
    let t = UiPalette::for_preset(ThemePreset::Classic).terminal;
    assert_eq!(t.bg, Color::from_rgb(0.11, 0.11, 0.12));
    assert_eq!(t.fg, Color::from_rgb(0.83, 0.84, 0.81));
    assert_eq!(t.cursor, Color::from_rgba(0.6, 0.6, 0.6, 0.8));
    assert_eq!(t.selection, Color::from_rgba(0.30, 0.50, 0.90, 0.40));
    assert_eq!(t.search_hit, Color::from_rgba(0.85, 0.65, 0.15, 0.35));
    // ANSI 16 与 `terminal_shader/helpers.rs::named_color_default()` 一致。
    assert_eq!(t.ansi[0], color!(0x000000));
    assert_eq!(t.ansi[1], color!(0xcc0000));
    assert_eq!(t.ansi[7], color!(0xd3d7cf));
    assert_eq!(t.ansi[15], color!(0xeeeeec));
}

/// classic 的状态栏色必须与改动前 `status_bar/chip.rs` 一致。
#[test]
fn classic_status_bar_matches_legacy() {
    let sb = UiPalette::for_preset(ThemePreset::Classic).status_bar;
    assert_eq!(sb.bar_bg.r, 43.0 / 255.0 * 0.85);
    assert_eq!(sb.bar_border, Color::from_rgba(0.90, 0.90, 0.90, 0.05));
    assert_eq!(sb.chip_border, Color::from_rgba(0.90, 0.90, 0.90, 0.30));
    assert_eq!(sb.chip_text, Color::from_rgb(0.90, 0.90, 0.90));
    assert_eq!(sb.chip_groups[0], Color::from_rgba(0.141176, 0.177882, 0.167294, 0.55));
    assert_eq!(sb.chip_groups[1], Color::from_rgba(0.181176, 0.145882, 0.187294, 0.55));
    assert_eq!(sb.chip_groups[2], Color::from_rgba(0.141176, 0.161882, 0.203294, 0.55));
    assert_eq!(sb.chip_groups[3], Color::from_rgba(0.165176, 0.169882, 0.179294, 0.55));
}

/// cyberpunk 的 16 色互不相同。
#[test]
fn cyberpunk_ansi_colors_are_distinct() {
    let ansi = UiPalette::for_preset(ThemePreset::Cyberpunk).terminal.ansi;
    for i in 0..ansi.len() {
        for j in (i + 1)..ansi.len() {
            assert_ne!(ansi[i], ansi[j], "ANSI {i} 与 {j} 撞色了");
        }
    }
}

/// cyberpunk 的 bright 色必须比对应 normal 色亮(否则终端里看不出层级)。
#[test]
fn cyberpunk_bright_colors_are_brighter() {
    let ansi = UiPalette::for_preset(ThemePreset::Cyberpunk).terminal.ansi;
    for i in 0..8 {
        assert!(
            luma(ansi[i + 8]) > luma(ansi[i]),
            "bright {i} 不比 normal {i} 亮:{:?} vs {:?}",
            ansi[i + 8],
            ansi[i]
        );
    }
}

/// cyberpunk 的关键色值必须与 HTML 视觉稿 `docs/preview/cyberpunk-ui.html`
/// 一致(两边都改了会红 —— 防止视觉稿与实现漂移)。
#[test]
fn cyberpunk_tokens_match_html_preview() {
    let p = UiPalette::for_preset(ThemePreset::Cyberpunk);
    assert_eq!(p.app.bg, color!(0x070A14));
    assert_eq!(p.app.surface, color!(0x0D1224));
    assert_eq!(p.app.surface_hover, color!(0x141B33));
    assert_eq!(p.app.hairline, color!(0x1E2A4A));
    assert_eq!(p.app.accent, color!(0x00E5FF));
    assert_eq!(p.app.accent_secondary, color!(0xFF2E97));
    assert_eq!(p.app.accent_tertiary, color!(0x8B5CF6));
    assert_eq!(p.app.success, color!(0x3DF5A4));
    assert_eq!(p.app.warning, color!(0xFFD166));
    assert_eq!(p.app.danger, color!(0xFF4D6D));
    assert_eq!(p.app.text_primary, color!(0xE6F7FF));
    assert_eq!(p.app.text_secondary, color!(0x7C8DB5));
    assert_eq!(p.app.text_dim, color!(0x4A5878));
    assert_eq!(p.terminal.bg, color!(0x05070F));
    assert_eq!(p.terminal.fg, color!(0xCFE9FF));
    assert_eq!(p.terminal.ansi[1], color!(0xFF3B6B));
    assert_eq!(p.terminal.ansi[5], color!(0xFF2E97));
    assert_eq!(p.terminal.ansi[6], color!(0x00E5FF));
    assert_eq!(p.terminal.ansi[15], color!(0xF2F7FF));
}

/// cyberpunk 模式下主应用与设置窗口用的是同一套外壳色 —— 两进程观感一致。
#[test]
fn cyberpunk_shells_are_identical() {
    let p = UiPalette::for_preset(ThemePreset::Cyberpunk);
    assert_eq!(p.app, p.settings);
}

/// Iced 主题映射:主强调/成功/警告/错误要对得上语义色。
#[test]
fn iced_theme_maps_palette() {
    let p = UiPalette::for_preset(ThemePreset::Cyberpunk);
    let theme = p.to_iced_theme();
    let ip = theme.palette();
    assert_eq!(ip.primary, p.app.accent);
    assert_eq!(ip.success, p.app.success);
    assert_eq!(ip.warning, p.app.warning);
    assert_eq!(ip.danger, p.app.danger);
    assert_eq!(ip.background, p.app.bg);
    assert_eq!(ip.text, p.app.text_primary);
}

/// 取色器预置色数量固定,且全部不透明。
#[test]
fn picker_swatches_are_opaque() {
    assert_eq!(PICKER_SWATCHES.len(), 12);
    for (color, label) in PICKER_SWATCHES {
        assert_eq!(color.a, 1.0, "{label} 应为不透明色");
        assert!(!label.is_empty());
    }
}

/// RGBA 往返转换无损。
#[test]
fn rgba_roundtrip() {
    let c = rgba(0.25, 0.5, 0.75, 0.5);
    assert_eq!(rgba_from_color(c), [0.25, 0.5, 0.75, 0.5]);
    assert_eq!(color_from_rgba(rgba_from_color(c)), c);
}
