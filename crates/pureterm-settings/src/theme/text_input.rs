use iced::widget::text_input;
use iced::{Background, Border, Color, Theme};

use super::{ACCENT, BORDER, CORNER_RADIUS, DANGER, SURFACE, TEXT_PRIMARY, TEXT_SECONDARY};

pub fn field(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    match status {
        text_input::Status::Active => text_input::Style {
            background: Background::Color(SURFACE),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: TEXT_SECONDARY,
            placeholder: TEXT_SECONDARY.scale_alpha(0.5),
            value: TEXT_PRIMARY,
            selection: ACCENT.scale_alpha(0.3),
        },
        text_input::Status::Hovered => text_input::Style {
            background: Background::Color(SURFACE),
            border: Border {
                color: ACCENT.scale_alpha(0.5),
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: TEXT_SECONDARY,
            placeholder: TEXT_SECONDARY.scale_alpha(0.5),
            value: TEXT_PRIMARY,
            selection: ACCENT.scale_alpha(0.3),
        },
        text_input::Status::Focused { .. } => text_input::Style {
            background: Background::Color(SURFACE),
            border: Border {
                color: ACCENT,
                width: 1.5,
                radius: CORNER_RADIUS.into(),
            },
            icon: TEXT_PRIMARY,
            placeholder: TEXT_SECONDARY.scale_alpha(0.5),
            value: TEXT_PRIMARY,
            selection: ACCENT.scale_alpha(0.3),
        },
        text_input::Status::Disabled => text_input::Style {
            background: Background::Color(SURFACE.scale_alpha(0.5)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: TEXT_SECONDARY.scale_alpha(0.3),
            placeholder: TEXT_SECONDARY.scale_alpha(0.3),
            value: TEXT_PRIMARY.scale_alpha(0.3),
            selection: Color::TRANSPARENT,
        },
    }
}

/// 与 `field` 相同，但边框为 DANGER 红色，用于无效输入（如非法 hex 色值）。
/// Disabled 状态保持透明（与 `field` 一致）。
pub fn field_invalid(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    match status {
        text_input::Status::Active => text_input::Style {
            background: Background::Color(SURFACE),
            border: Border {
                color: DANGER,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: TEXT_SECONDARY,
            placeholder: TEXT_SECONDARY.scale_alpha(0.5),
            value: TEXT_PRIMARY,
            selection: ACCENT.scale_alpha(0.3),
        },
        text_input::Status::Hovered => text_input::Style {
            background: Background::Color(SURFACE),
            border: Border {
                color: DANGER.scale_alpha(0.7),
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: TEXT_SECONDARY,
            placeholder: TEXT_SECONDARY.scale_alpha(0.5),
            value: TEXT_PRIMARY,
            selection: ACCENT.scale_alpha(0.3),
        },
        text_input::Status::Focused { .. } => text_input::Style {
            background: Background::Color(SURFACE),
            border: Border {
                color: DANGER,
                width: 1.5,
                radius: CORNER_RADIUS.into(),
            },
            icon: TEXT_PRIMARY,
            placeholder: TEXT_SECONDARY.scale_alpha(0.5),
            value: TEXT_PRIMARY,
            selection: ACCENT.scale_alpha(0.3),
        },
        text_input::Status::Disabled => text_input::Style {
            background: Background::Color(SURFACE.scale_alpha(0.5)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: TEXT_SECONDARY.scale_alpha(0.3),
            placeholder: TEXT_SECONDARY.scale_alpha(0.3),
            value: TEXT_PRIMARY.scale_alpha(0.3),
            selection: Color::TRANSPARENT,
        },
    }
}
