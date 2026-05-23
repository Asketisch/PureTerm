use iced::widget::{button, container, text, Column};
use iced::{Element, Length, Padding, Theme};

use crate::app::Message;
use crate::pages::Page;
use crate::theme::{self, PADDING, SIDEBAR_WIDTH, SPACING};

pub fn view(current_page: Page) -> Element<'static, Message> {
    let items = vec![
        ("General", Page::General),
        ("Appearance", Page::Appearance),
        ("Colors", Page::Colors),
        ("Terminal", Page::Terminal),
        ("SSH Profiles", Page::SshProfiles),
        ("Shortcuts", Page::Keybindings),
    ];

    let buttons: Vec<Element<Message>> = items
        .into_iter()
        .map(|(label, page)| {
            let selected = current_page == page;
            button(
                text(label).size(theme::FONT_SIZE),
            )
            .width(Length::Fill)
            .padding(Padding::from([8, PADDING as u16]))
            .style(move |t: &Theme, s| theme::button::sidebar_item(t, s, selected))
            .on_press(Message::NavigateTo(page))
            .into()
        })
        .collect();

    container(
        Column::with_children(buttons)
            .spacing(SPACING / 2.0)
            .padding(Padding::from([PADDING as u16, PADDING as u16 / 2])),
    )
    .width(SIDEBAR_WIDTH)
    .height(Length::Fill)
    .style(theme::container::sidebar)
    .into()
}
