//! GAP-72：外壳样式助手 —— 把散落在各 `view_*` 里的内联
//! `container::Style` / `Border` / `Shadow` 收敛成可复用函数。
//!
//! 本模块是**主应用**专用的外壳样式助手;设置窗口有自己的一套控件样式
//! (`pureterm-settings/src/theme/`),但两者共用同一份调色板
//! (`pureterm_settings::theme::palette`),观感不会漂移。
//!
//! 形态规则(见 GAP-72 spec §3.3):
//! - 圆角统一 8px(面板/标签/浮层),chip 4px;
//! - 面板 = 底色 + 1px 发丝线描边;
//! - 浮层 = 高层底色 + 强调色描边 + 12px 外发光;
//! - **发光只用于激活 tab / 焦点输入框 / 浮层面板三处**,避免每帧多处
//!   blur 绘制拖垮帧率。

use iced::widget::container;
use iced::{Background, Border, Shadow, Vector};

use pureterm_settings::theme::palette::ShellTokens;

/// 面板 / 浮层的统一圆角。
pub const RADIUS: f32 = 8.0;
/// chip 的圆角(比面板更小,视觉上更"轻")。
pub const CHIP_RADIUS: f32 = 4.0;
/// 霓虹外发光的模糊半径。
pub const GLOW_BLUR: f32 = 12.0;

/// 发丝线描边(1px)。
pub fn hairline_border(tokens: ShellTokens) -> Border {
    Border {
        color: tokens.hairline,
        width: 1.0,
        radius: RADIUS.into(),
    }
}

/// 霓虹发光阴影(零偏移纯外发光)。
pub fn focus_shadow(tokens: ShellTokens) -> Shadow {
    Shadow {
        color: tokens.glow,
        offset: Vector::ZERO,
        blur_radius: GLOW_BLUR,
    }
}

/// 普通面板样式(底色 + 发丝线描边)。
pub fn panel(tokens: ShellTokens, elevated: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if elevated {
            tokens.elevated
        } else {
            tokens.surface
        })),
        text_color: Some(tokens.text_primary),
        border: hairline_border(tokens),
        snap: false,
        shadow: Shadow::default(),
    }
}

/// 玻璃面板样式(半透明底 + 发丝线描边)。
pub fn glass_panel(tokens: ShellTokens) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens.glass)),
        text_color: Some(tokens.text_primary),
        border: hairline_border(tokens),
        snap: false,
        shadow: Shadow::default(),
    }
}

/// 浮层面板样式(高层底 + 强调色描边 + 外发光)。
pub fn overlay_panel(tokens: ShellTokens) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens.overlay_bg)),
        text_color: Some(tokens.text_primary),
        border: Border {
            color: tokens.overlay_border,
            width: 1.0,
            radius: RADIUS.into(),
        },
        snap: false,
        shadow: focus_shadow(tokens),
    }
}

/// 激活项样式(高亮底 + 2px 左侧强调竖条)。
pub fn active_item(tokens: ShellTokens) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens.highlight)),
        text_color: Some(tokens.accent),
        border: Border {
            color: tokens.accent,
            width: 2.0,
            radius: CHIP_RADIUS.into(),
        },
        snap: false,
        shadow: Shadow::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pureterm_settings::theme::palette::{ThemePreset, UiPalette};

    fn cyber() -> ShellTokens {
        UiPalette::for_preset(ThemePreset::Cyberpunk).app
    }

    #[test]
    fn panel_uses_tokens() {
        let t = cyber();
        let style = panel(t, false);
        assert_eq!(style.background, Some(Background::Color(t.surface)));
        assert_eq!(style.border.color, t.hairline);
        assert_eq!(style.border.width, 1.0);
    }

    #[test]
    fn elevated_panel_uses_elevated_color() {
        let t = cyber();
        let style = panel(t, true);
        assert_eq!(style.background, Some(Background::Color(t.elevated)));
    }

    #[test]
    fn overlay_panel_has_glow() {
        let t = cyber();
        let style = overlay_panel(t);
        assert_eq!(style.background, Some(Background::Color(t.overlay_bg)));
        assert_eq!(style.border.color, t.overlay_border);
        assert_eq!(style.shadow.color, t.glow);
        assert_eq!(style.shadow.blur_radius, GLOW_BLUR);
        assert_eq!(style.shadow.offset, Vector::ZERO);
    }

    #[test]
    fn active_item_has_accent_bar() {
        let t = cyber();
        let style = active_item(t);
        assert_eq!(style.background, Some(Background::Color(t.highlight)));
        assert_eq!(style.border.color, t.accent);
        assert_eq!(style.border.width, 2.0);
    }

    #[test]
    fn glass_panel_is_translucent() {
        let t = cyber();
        assert!(t.glass.a < 1.0);
        let style = glass_panel(t);
        assert_eq!(style.background, Some(Background::Color(t.glass)));
    }
}
