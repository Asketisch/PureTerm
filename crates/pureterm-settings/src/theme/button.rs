use iced::widget::button;
use iced::{Background, Border, Color, Shadow, Theme};

use super::tokens;
use super::{CORNER_RADIUS, darken, lighten};

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let _ = theme;
    let t = tokens();
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(t.accent)),
            text_color: t.bg,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(t.accent_hover)),
            text_color: t.bg,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(darken(t.accent, 0.05))),
            text_color: t.bg,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(t.accent.scale_alpha(0.3))),
            text_color: t.bg.scale_alpha(0.5),
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
    let t = tokens();
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(t.surface)),
            text_color: t.text_primary,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(t.surface_hover)),
            text_color: lighten(t.text_primary, 0.1),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(t.surface)),
            text_color: darken(t.text_primary, 0.1),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(t.surface.scale_alpha(0.5))),
            text_color: t.text_secondary.scale_alpha(0.5),
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
    let t = tokens();
    let bg = if selected { t.surface_hover } else { Color::TRANSPARENT };
    let left_border = if selected { t.accent } else { Color::TRANSPARENT };

    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(bg)),
            text_color: if selected { t.text_primary } else { t.text_secondary },
            border: Border {
                color: left_border,
                width: if selected { 2.0 } else { 0.0 },
                radius: CORNER_RADIUS.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(t.surface_hover)),
            text_color: t.text_primary,
            border: Border {
                color: if selected { t.accent } else { Color::TRANSPARENT },
                width: if selected { 2.0 } else { 0.0 },
                radius: CORNER_RADIUS.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        },
        _ => button::Style {
            background: Some(Background::Color(bg)),
            text_color: t.text_secondary,
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
    let t = tokens();
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(t.danger)),
            text_color: t.text_primary,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(lighten(t.danger, 0.1))),
            text_color: t.text_primary,
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
        _ => button::Style {
            background: Some(Background::Color(t.danger.scale_alpha(0.5))),
            text_color: t.text_primary.scale_alpha(0.5),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        },
    }
}
