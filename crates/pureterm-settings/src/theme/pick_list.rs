use iced::widget::pick_list;
use iced::{Background, Border, Shadow, Theme};

use super::tokens;
use super::CORNER_RADIUS;

pub fn field(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let t = tokens();
    match status {
        pick_list::Status::Active => pick_list::Style {
            text_color: t.text_primary,
            placeholder_color: t.text_secondary.scale_alpha(0.5),
            handle_color: t.text_secondary,
            background: Background::Color(t.surface),
            border: Border {
                color: t.hairline,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
        },
        pick_list::Status::Hovered => pick_list::Style {
            text_color: t.text_primary,
            placeholder_color: t.text_secondary,
            handle_color: t.text_primary,
            background: Background::Color(t.surface_hover),
            border: Border {
                color: t.accent.scale_alpha(0.5),
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
        },
        pick_list::Status::Opened { .. } => pick_list::Style {
            text_color: t.text_primary,
            placeholder_color: t.text_secondary,
            handle_color: t.accent,
            background: Background::Color(t.surface_hover),
            border: Border {
                color: t.accent,
                width: 1.5,
                radius: CORNER_RADIUS.into(),
            },
        },
    }
}

pub fn menu(_theme: &Theme) -> iced::overlay::menu::Style {
    let t = tokens();
    iced::overlay::menu::Style {
        background: Background::Color(t.surface),
        border: Border {
            color: t.hairline,
            width: 1.0,
            radius: CORNER_RADIUS.into(),
        },
        text_color: t.text_primary,
        selected_text_color: t.text_primary,
        selected_background: Background::Color(t.accent.scale_alpha(0.2)),
        shadow: Shadow::default(),
    }
}
