use iced::widget::container;
use iced::{Background, Border, Theme};

use super::{BACKGROUND, CORNER_RADIUS, SURFACE};

pub fn sidebar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        ..container::Style::default()
    }
}

pub fn page(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BACKGROUND)),
        ..container::Style::default()
    }
}

pub fn card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            radius: CORNER_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}
