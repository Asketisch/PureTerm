use iced::widget::{button, container, text, Column};
use iced::{Element, Length, Padding, Theme};

use crate::app::Message;
use crate::i18n::I18n;
use crate::pages::Page;
use crate::theme::{self, PADDING, SIDEBAR_WIDTH, SPACING};

pub fn view(current_page: Page, i18n: &I18n) -> Element<'static, Message> {
    let items = vec![
        (i18n.t("sidebar.general"), Page::General),
        (i18n.t("sidebar.appearance"), Page::Appearance),
        (i18n.t("sidebar.colors"), Page::Colors),
        (i18n.t("sidebar.terminal"), Page::Terminal),
        (i18n.t("sidebar.ssh_profiles"), Page::SshProfiles),
        (i18n.t("sidebar.shortcuts"), Page::Keybindings),
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
