use iced::widget::rule;
use iced::Theme;

use super::tokens;

pub fn separator(_theme: &Theme) -> rule::Style {
    rule::Style {
        color: tokens().hairline,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}
