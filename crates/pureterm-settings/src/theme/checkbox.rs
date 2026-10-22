use iced::widget::checkbox;
use iced::{Background, Border, Color, Theme};

use super::tokens;
use super::CORNER_RADIUS;

pub fn field(_theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let t = tokens();
    match status {
        checkbox::Status::Active { is_checked } => checkbox::Style {
            background: Background::Color(if is_checked { t.accent } else { t.surface }),
            icon_color: if is_checked { t.bg } else { Color::TRANSPARENT },
            border: Border {
                color: if is_checked { t.accent } else { t.hairline },
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            text_color: Some(t.text_primary),
        },
        checkbox::Status::Hovered { is_checked } => checkbox::Style {
            background: Background::Color(if is_checked { t.accent_hover } else { t.surface }),
            icon_color: if is_checked { t.bg } else { Color::TRANSPARENT },
            border: Border {
                color: t.accent,
                width: 1.5,
                radius: CORNER_RADIUS.into(),
            },
            text_color: Some(t.text_primary),
        },
        checkbox::Status::Disabled { is_checked } => checkbox::Style {
            background: Background::Color(t.surface.scale_alpha(0.3)),
            icon_color: if is_checked {
                t.bg.scale_alpha(0.3)
            } else {
                Color::TRANSPARENT
            },
            border: Border {
                color: t.hairline.scale_alpha(0.3),
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            text_color: Some(t.text_primary.scale_alpha(0.3)),
        },
    }
}
