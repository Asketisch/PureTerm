use iced::widget::pick_list;
use iced::{Background, Border, Shadow, Theme};

use super::{ACCENT, BORDER, CORNER_RADIUS, SURFACE, SURFACE_HOVER, TEXT_PRIMARY, TEXT_SECONDARY};

pub fn field(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    match status {
        pick_list::Status::Active => pick_list::Style {
            text_color: TEXT_PRIMARY,
            placeholder_color: TEXT_SECONDARY.scale_alpha(0.5),
            handle_color: TEXT_SECONDARY,
            background: Background::Color(SURFACE),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
        },
        pick_list::Status::Hovered => pick_list::Style {
            text_color: TEXT_PRIMARY,
            placeholder_color: TEXT_SECONDARY,
            handle_color: TEXT_PRIMARY,
            background: Background::Color(SURFACE_HOVER),
            border: Border {
                color: ACCENT.scale_alpha(0.5),
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
        },
        pick_list::Status::Opened { .. } => pick_list::Style {
            text_color: TEXT_PRIMARY,
            placeholder_color: TEXT_SECONDARY,
            handle_color: ACCENT,
            background: Background::Color(SURFACE_HOVER),
            border: Border {
                color: ACCENT,
                width: 1.5,
                radius: CORNER_RADIUS.into(),
            },
        },
    }
}

pub fn menu(_theme: &Theme) -> iced::overlay::menu::Style {
    iced::overlay::menu::Style {
        background: Background::Color(SURFACE),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: CORNER_RADIUS.into(),
        },
        text_color: TEXT_PRIMARY,
        selected_text_color: TEXT_PRIMARY,
        selected_background: Background::Color(ACCENT.scale_alpha(0.2)),
        shadow: Shadow::default(),
    }
}
