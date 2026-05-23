use iced::widget::button;
use iced::{Background, Border, Color, Shadow, Theme};

use super::{
    ACCENT, ACCENT_HOVER, BACKGROUND, CORNER_RADIUS, DANGER, SURFACE, SURFACE_HOVER,
    TEXT_PRIMARY, TEXT_SECONDARY, darken, lighten,
};

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let _ = theme;
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(ACCENT)),
            text_color: BACKGROUND,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(ACCENT_HOVER)),
            text_color: BACKGROUND,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(darken(ACCENT, 0.05))),
            text_color: BACKGROUND,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(ACCENT.scale_alpha(0.3))),
            text_color: BACKGROUND.scale_alpha(0.5),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
    }
}

pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let _ = theme;
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(SURFACE)),
            text_color: TEXT_PRIMARY,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(SURFACE_HOVER)),
            text_color: lighten(TEXT_PRIMARY, 0.1),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(SURFACE)),
            text_color: darken(TEXT_PRIMARY, 0.1),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(SURFACE.scale_alpha(0.5))),
            text_color: TEXT_SECONDARY.scale_alpha(0.5),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
    }
}

pub fn sidebar_item(_theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let bg = if selected { SURFACE_HOVER } else { Color::TRANSPARENT };
    let left_border = if selected { ACCENT } else { Color::TRANSPARENT };

    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(bg)),
            text_color: if selected { TEXT_PRIMARY } else { TEXT_SECONDARY },
            border: Border {
                color: left_border,
                width: if selected { 2.0 } else { 0.0 },
                radius: CORNER_RADIUS.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(SURFACE_HOVER)),
            text_color: TEXT_PRIMARY,
            border: Border {
                color: if selected { ACCENT } else { Color::TRANSPARENT },
                width: if selected { 2.0 } else { 0.0 },
                radius: CORNER_RADIUS.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        },
        _ => button::Style {
            background: Some(Background::Color(bg)),
            text_color: TEXT_SECONDARY,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
    }
}

pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let _ = theme;
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(DANGER)),
            text_color: TEXT_PRIMARY,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(lighten(DANGER, 0.1))),
            text_color: TEXT_PRIMARY,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        _ => button::Style {
            background: Some(Background::Color(DANGER.scale_alpha(0.5))),
            text_color: TEXT_PRIMARY.scale_alpha(0.5),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
    }
}
