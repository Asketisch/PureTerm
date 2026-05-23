use iced::widget::rule;
use iced::Theme;

use super::BORDER;

pub fn separator(_theme: &Theme) -> rule::Style {
    rule::Style {
        color: BORDER,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}
