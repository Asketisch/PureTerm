use iced::widget::container;
use iced::{Background, Border, Element, Length};

use crate::theme::CORNER_RADIUS;

pub fn view<'a, Message: 'a>(color: iced::Color, size: f32) -> Element<'a, Message> {
    container("")
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(color)),
            // PureTerm: 加可见边框，让深色色块（如 background #181818）
            // 在暗色/磨砂设置窗口里也能被看见，并把色块「圈」成可识别的色块。
            border: Border {
                radius: CORNER_RADIUS.into(),
                width: 1.0,
                color: crate::theme::tokens().hairline,
            },
            ..container::Style::default()
        })
        .into()
}
