use iced::widget::slider;
use iced::{Background, Border, Color, Theme};

use super::{ACCENT, ACCENT_HOVER, BORDER};

pub fn field(_theme: &Theme, status: slider::Status) -> slider::Style {
    let rail = slider::Rail {
        backgrounds: (
            Background::Color(ACCENT),
            Background::Color(BORDER),
        ),
        width: 4.0,
        border: Border {
            radius: 2.0.into(),
            ..Border::default()
        },
    };

    let handle_style = |color: Color| slider::Handle {
        shape: slider::HandleShape::Circle { radius: 7.0 },
        background: Background::Color(color),
        border_width: 0.0,
        border_color: Color::TRANSPARENT,
    };

    match status {
        slider::Status::Active => slider::Style {
            rail,
            handle: handle_style(ACCENT),
        },
        slider::Status::Hovered => slider::Style {
            rail,
            handle: handle_style(ACCENT_HOVER),
        },
        slider::Status::Dragged => slider::Style {
            rail,
            handle: handle_style(ACCENT_HOVER),
        },
    }
}
