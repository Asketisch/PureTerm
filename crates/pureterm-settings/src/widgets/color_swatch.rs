use iced::widget::container;
use iced::{Background, Border, Element, Length};

use crate::theme::CORNER_RADIUS;

pub fn view<'a, Message: 'a>(color: iced::Color, size: f32) -> Element<'a, Message> {
    container("")
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: CORNER_RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}
