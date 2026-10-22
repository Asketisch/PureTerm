use iced::widget::container;
use iced::{Background, Border, Color, Theme};

use super::tokens;
use super::CORNER_RADIUS;

pub fn sidebar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens().surface)),
        ..container::Style::default()
    }
}

/// PureTerm: 整页容器改成完全透明，让磨砂层（window.blur）从底层透出。
/// 实际可见的背景由调色板 `Palette::background` 的 alpha 决定（见
/// `theme::pureterm_theme_with_opacity`）。
pub fn page(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.0,
            ..tokens().bg
        })),
        ..container::Style::default()
    }
}

pub fn card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens().surface)),
        border: Border {
            radius: CORNER_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}
