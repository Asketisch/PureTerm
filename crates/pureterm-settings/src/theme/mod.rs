pub mod button;
pub mod checkbox;
pub mod container;
pub mod pick_list;
pub mod rule;
pub mod scrollable;
pub mod slider;
pub mod text_input;

use iced::{Color, Theme, color};

pub const CORNER_RADIUS: f32 = 4.0;
pub const FONT_SIZE: f32 = 13.0;
pub const SIDEBAR_WIDTH: f32 = 180.0;
pub const PADDING: f32 = 12.0;
pub const SPACING: f32 = 8.0;

pub const BACKGROUND: Color = color!(0x2b292d);
pub const SURFACE: Color = color!(0x383539);
pub const SURFACE_HOVER: Color = color!(0x44414a);
pub const BORDER: Color = color!(0x504d55);
pub const TEXT_PRIMARY: Color = color!(0xfecdb2);
pub const TEXT_SECONDARY: Color = color!(0xb0a392);
pub const ACCENT: Color = color!(0xd3869b);
pub const ACCENT_HOVER: Color = color!(0xe09baf);
pub const SUCCESS: Color = color!(0x90a959);
pub const WARNING: Color = color!(0xf4bf75);
pub const DANGER: Color = color!(0xac4242);

pub fn pureterm_theme() -> Theme {
    Theme::custom(
        "PureTerm".to_string(),
        iced::theme::Palette {
            background: BACKGROUND,
            text: TEXT_PRIMARY,
            primary: ACCENT,
            success: SUCCESS,
            danger: DANGER,
            warning: WARNING,
        },
    )
}

/// PureTerm：根据用户配置的 background opacity 动态生成 Theme。
/// `appearance.opacity` 越低，调色板背景 alpha 越小，越能透出磨砂层。
/// 注意：Iced 0.14 的 `window::Settings.transparent/blur` 一旦创建不可改，
/// 所以该 opacity 的实际效果在「设置窗口下次启动」时才会呈现。
pub fn pureterm_theme_with_opacity(opacity: f32) -> Theme {
    let bg = Color {
        a: opacity.clamp(0.0, 1.0),
        ..BACKGROUND
    };
    Theme::custom(
        "PureTerm".to_string(),
        iced::theme::Palette {
            background: bg,
            text: TEXT_PRIMARY,
            primary: ACCENT,
            success: SUCCESS,
            danger: DANGER,
            warning: WARNING,
        },
    )
}

pub fn lighten(color: Color, amount: f32) -> Color {
    Color {
        r: (color.r + amount).min(1.0),
        g: (color.g + amount).min(1.0),
        b: (color.b + amount).min(1.0),
        a: color.a,
    }
}

pub fn darken(color: Color, amount: f32) -> Color {
    Color {
        r: (color.r - amount).max(0.0),
        g: (color.g - amount).max(0.0),
        b: (color.b - amount).max(0.0),
        a: color.a,
    }
}
