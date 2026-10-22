use iced::widget::slider;
use iced::{Background, Border, Color, Theme};

use super::tokens;

pub fn field(_theme: &Theme, status: slider::Status) -> slider::Style {
    let t = tokens();
    let rail = slider::Rail {
        backgrounds: (
            Background::Color(t.accent),
            Background::Color(t.hairline),
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
            handle: handle_style(t.accent),
        },
        slider::Status::Hovered => slider::Style {
            rail,
            handle: handle_style(t.accent_hover),
        },
        slider::Status::Dragged => slider::Style {
            rail,
            handle: handle_style(t.accent_hover),
        },
    }
}
