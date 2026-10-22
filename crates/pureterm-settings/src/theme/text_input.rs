use iced::widget::text_input;
use iced::{Background, Border, Color, Theme};

use super::tokens;
use super::CORNER_RADIUS;

pub fn field(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let t = tokens();
    match status {
        text_input::Status::Active => text_input::Style {
            background: Background::Color(t.surface),
            border: Border {
                color: t.hairline,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: t.text_secondary,
            placeholder: t.text_secondary.scale_alpha(0.5),
            value: t.text_primary,
            selection: t.accent.scale_alpha(0.3),
        },
        text_input::Status::Hovered => text_input::Style {
            background: Background::Color(t.surface),
            border: Border {
                color: t.accent.scale_alpha(0.5),
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: t.text_secondary,
            placeholder: t.text_secondary.scale_alpha(0.5),
            value: t.text_primary,
            selection: t.accent.scale_alpha(0.3),
        },
        text_input::Status::Focused { .. } => text_input::Style {
            background: Background::Color(t.surface),
            border: Border {
                color: t.accent,
                width: 1.5,
                radius: CORNER_RADIUS.into(),
            },
            icon: t.text_primary,
            placeholder: t.text_secondary.scale_alpha(0.5),
            value: t.text_primary,
            selection: t.accent.scale_alpha(0.3),
        },
        text_input::Status::Disabled => text_input::Style {
            background: Background::Color(t.surface.scale_alpha(0.5)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: t.text_secondary.scale_alpha(0.3),
            placeholder: t.text_secondary.scale_alpha(0.3),
            value: t.text_primary.scale_alpha(0.3),
            selection: Color::TRANSPARENT,
        },
    }
}

/// 与 `field` 相同，但边框为 DANGER 红色，用于无效输入（如非法 hex 色值）。
/// Disabled 状态保持透明（与 `field` 一致）。
pub fn field_invalid(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let t = tokens();
    match status {
        text_input::Status::Active => text_input::Style {
            background: Background::Color(t.surface),
            border: Border {
                color: t.danger,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: t.text_secondary,
            placeholder: t.text_secondary.scale_alpha(0.5),
            value: t.text_primary,
            selection: t.accent.scale_alpha(0.3),
        },
        text_input::Status::Hovered => text_input::Style {
            background: Background::Color(t.surface),
            border: Border {
                color: t.danger.scale_alpha(0.7),
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: t.text_secondary,
            placeholder: t.text_secondary.scale_alpha(0.5),
            value: t.text_primary,
            selection: t.accent.scale_alpha(0.3),
        },
        text_input::Status::Focused { .. } => text_input::Style {
            background: Background::Color(t.surface),
            border: Border {
                color: t.danger,
                width: 1.5,
                radius: CORNER_RADIUS.into(),
            },
            icon: t.text_primary,
            placeholder: t.text_secondary.scale_alpha(0.5),
            value: t.text_primary,
            selection: t.accent.scale_alpha(0.3),
        },
        text_input::Status::Disabled => text_input::Style {
            background: Background::Color(t.surface.scale_alpha(0.5)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            icon: t.text_secondary.scale_alpha(0.3),
            placeholder: t.text_secondary.scale_alpha(0.3),
            value: t.text_primary.scale_alpha(0.3),
            selection: Color::TRANSPARENT,
        },
    }
}
