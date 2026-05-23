use iced::widget::checkbox;
use iced::{Background, Border, Color, Theme};

use super::{ACCENT, ACCENT_HOVER, BORDER, CORNER_RADIUS, SURFACE, TEXT_PRIMARY};

pub fn field(_theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    match status {
        checkbox::Status::Active { is_checked } => checkbox::Style {
            background: Background::Color(if is_checked { ACCENT } else { SURFACE }),
            icon_color: if is_checked { Color::WHITE } else { Color::TRANSPARENT },
            border: Border {
                color: if is_checked { ACCENT } else { BORDER },
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            text_color: Some(TEXT_PRIMARY),
        },
        checkbox::Status::Hovered { is_checked } => checkbox::Style {
            background: Background::Color(if is_checked { ACCENT_HOVER } else { SURFACE }),
            icon_color: if is_checked { Color::WHITE } else { Color::TRANSPARENT },
            border: Border {
                color: ACCENT,
                width: 1.5,
                radius: CORNER_RADIUS.into(),
            },
            text_color: Some(TEXT_PRIMARY),
        },
        checkbox::Status::Disabled { is_checked } => checkbox::Style {
            background: Background::Color(SURFACE.scale_alpha(0.3)),
            icon_color: if is_checked {
                Color::WHITE.scale_alpha(0.3)
            } else {
                Color::TRANSPARENT
            },
            border: Border {
                color: BORDER.scale_alpha(0.3),
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            text_color: Some(TEXT_PRIMARY.scale_alpha(0.3)),
        },
    }
}
