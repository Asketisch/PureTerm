//! 设置窗口主题 —— GAP-72 起由 [`palette::UiPalette`] 驱动。
//!
//! 两种取色路径:
//! - **样式函数**(button / text_input / …):签名由 Iced 规定,只能拿到
//!   `&Theme`,拿不到我们的语义色表;因此用**进程级全局** [`tokens()`]
//!   承载当前外壳色。设置窗口是单窗口单进程,全局唯一且启动期写入一次,
//!   语义上等价于「窗口主题」。
//! - **页面代码**(app.rs / pages/*):直接 `theme::tokens().xxx` 取色。
//!
//! 主应用(`pureterm` crate)也调用 [`set_tokens`] —— 它在进程内渲染
//! 设置窗口(`iced_app.rs` 的 `theme()` 会切到设置窗口),必须与独立
//! 设置进程呈现同一套观感。

pub mod button;
pub mod caption;
pub mod checkbox;
pub mod container;
pub mod palette;
pub mod pick_list;
pub mod rule;
pub mod scrollable;
pub mod slider;
pub mod text_input;
// `titlebar` / `titlebar_view` 只被**主应用**(pureterm crate)使用:
// 设置窗口保留系统装饰(工具型窗口需要系统关闭按钮),不画自绘标题栏。
// 但本 crate 的 `src/main.rs` 会把整棵模块树再编译一遍当成 bin,
// 于是这些 pub 项在 bin 目标里被判 dead_code —— 属于重复编译的假象,
// 不是真的没人用。
#[allow(dead_code)]
pub mod titlebar;
#[allow(dead_code)]
pub mod titlebar_view;

use std::sync::RwLock;

use iced::{Color, Theme};

pub use palette::{ShellTokens, ThemePreset, UiPalette};
pub use titlebar::{TitleBarStyle, TRAFFIC_LIGHT_INSET, TITLE_BAR_HEIGHT};

pub const CORNER_RADIUS: f32 = 4.0;
pub const FONT_SIZE: f32 = 13.0;
pub const SIDEBAR_WIDTH: f32 = 180.0;
pub const PADDING: f32 = 12.0;
pub const SPACING: f32 = 8.0;

/// 进程级当前外壳色。`None` = 尚未设置(回落到 classic)。
static TOKENS: RwLock<Option<ShellTokens>> = RwLock::new(None);

/// 设置当前外壳色(启动期调用一次;设置窗口里切换预设时再调一次)。
pub fn set_tokens(tokens: ShellTokens) {
    if let Ok(mut guard) = TOKENS.write() {
        *guard = Some(tokens);
    }
}

/// 当前外壳色。未设置时回落到 classic 预设,保证任何调用点都不会 panic。
pub fn tokens() -> ShellTokens {
    TOKENS
        .read()
        .ok()
        .and_then(|guard| *guard)
        .unwrap_or_else(|| UiPalette::for_preset(ThemePreset::Classic).settings)
}

/// 设置窗口的 Iced 主题(当前外壳色)。
pub fn pureterm_theme() -> Theme {
    tokens().to_iced_theme()
}

/// 指定预设的 Iced 主题(不改变进程级全局)。
///
/// 主应用按配置里的预设构建窗口主题时用它;设置进程内部走
/// [`tokens()`] 实时换肤,不需要这个入口。
#[allow(dead_code)]
pub fn pureterm_theme_for(preset: ThemePreset) -> Theme {
    UiPalette::for_preset(preset).settings.to_iced_theme()
}

/// PureTerm：根据用户配置的 background opacity 动态生成 Theme。
/// `appearance.opacity` 越低，调色板背景 alpha 越小，越能透出磨砂层。
/// 注意：Iced 0.14 的 `window::Settings.transparent/blur` 一旦创建不可改，
/// 所以该 opacity 的实际效果在「设置窗口下次启动」时才会呈现。
pub fn pureterm_theme_with_opacity(opacity: f32) -> Theme {
    let t = tokens();
    let bg = Color {
        a: opacity.clamp(0.0, 1.0),
        ..t.bg
    };
    Theme::custom(
        "PureTerm".to_string(),
        iced::theme::Palette {
            background: bg,
            text: t.text_primary,
            primary: t.accent,
            success: t.success,
            danger: t.danger,
            warning: t.warning,
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
