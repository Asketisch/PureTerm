use iced::widget::{container, scrollable};
use iced::{Background, Border, Color, Shadow, Theme};

use super::tokens;
use super::CORNER_RADIUS;

pub fn dark(_theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let t = tokens();
    let rail = scrollable::Rail {
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(t.hairline),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
        },
    };

    match status {
        scrollable::Status::Active { .. } => scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                shadow: Shadow::default(),
                icon: Color::TRANSPARENT,
            },
        },
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered,
            ..
        } => {
            let hovered_rail = scrollable::Rail {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border::default(),
                scroller: scrollable::Scroller {
                    background: Background::Color(if is_vertical_scrollbar_hovered {
                        t.accent
                    } else {
                        t.surface_hover
                    }),
                    border: Border {
                        radius: CORNER_RADIUS.into(),
                        ..Border::default()
                    },
                },
            };
            scrollable::Style {
                container: container::Style::default(),
                vertical_rail: hovered_rail,
                horizontal_rail: rail,
                gap: None,
                auto_scroll: scrollable::AutoScroll {
                    background: Background::Color(Color::TRANSPARENT),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    icon: Color::TRANSPARENT,
                },
            }
        }
        scrollable::Status::Dragged { .. } => {
            let drag_rail = scrollable::Rail {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border::default(),
                scroller: scrollable::Scroller {
                    background: Background::Color(t.accent),
                    border: Border {
                        radius: CORNER_RADIUS.into(),
                        ..Border::default()
                    },
                },
            };
            scrollable::Style {
                container: container::Style::default(),
                vertical_rail: drag_rail,
                horizontal_rail: rail,
                gap: None,
                auto_scroll: scrollable::AutoScroll {
                    background: Background::Color(Color::TRANSPARENT),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    icon: Color::TRANSPARENT,
                },
            }
        }
    }
}
