use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme};

use super::data::{BlocksTabData, DirFindData, GitTabData, PortScannerTabData, RemoteFilesTabData, SurfaceFindData, ToolbeltTabData};
use pureterm_settings::theme::palette::ShellTokens;

use crate::iced_app::Message;
use crate::sidebar::RightMode;

pub(crate) fn view_find<'a>(
    tokens: ShellTokens,
    surface: Option<SurfaceFindData<'a>>,
    dir: Option<DirFindData<'a>>,
    _width: f32,
    _height: f32,
) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = tokens.text_secondary;
    let mut col = column![].spacing(8);

    col = col.push(text("In Surface (⌘F)").size(12).color(dim));

    if let Some(s) = surface {
        let input = text_input("Find…", s.query)
            .on_input(Message::SurfaceSearchQuery)
            .on_submit(Message::SurfaceSearchConfirm)
            .width(Length::Fill);
        col = col.push(input);

        let toggle = |label: &str, active: bool, msg: Message| {
            let caption = if active {
                format!("●{label}")
            } else {
                label.to_string()
            };
            button(text(caption).size(11)).on_press(msg)
        };
        col = col.push(
            row![
                toggle("Aa", s.case_sensitive, Message::SurfaceSearchToggleCase),
                toggle(".*", s.regex_mode, Message::SurfaceSearchToggleRegex),
                toggle(r#""""#, s.whole_word, Message::SurfaceSearchToggleWholeWord),
                toggle("↻", s.follow_tail, Message::SurfaceSearchToggleFollow),
            ]
            .spacing(4),
        );
        col = col.push(
            row![
                button(text("Next").size(11))
                    .on_press(Message::SurfaceSearchNext)
                    .style(button::secondary),
                button(text("Prev").size(11))
                    .on_press(Message::SurfaceSearchPrev)
                    .style(button::secondary),
            ]
            .spacing(4),
        );
    } else {
        col = col.push(
            button(text("Open surface search").size(11))
                .on_press(Message::SurfaceSearchOpen)
                .style(button::secondary),
        );
    }

    col = col.push(space().height(8));
    col = col.push(text("In Directory (⌘⇧F)").size(12).color(dim));

    if let Some(d) = dir {
        col = col.push(
            text_input("Search in working directory…", d.query)
                .on_input(Message::DirSearchQuery)
                .width(Length::Fill),
        );
        if d.running {
            col = col.push(text("searching…").size(11).color(dim));
        }
        if let Some(err) = d.error {
            col = col.push(text(err).size(11).color(tokens.error_text_soft));
        }
        if !d.results.is_empty() {
            let mut list = column![].spacing(2);
            for (idx, m) in d.results.iter().take(100).enumerate() {
                let label = format!("{}:{}  {}", m.path, m.line, m.text.trim());
                list = list.push(
                    button(text(label).size(10))
                        .on_press(Message::DirSearchSelect(idx))
                        .width(Length::Fill),
                );
            }
            col = col.push(list);
        }
    } else {
        col = col.push(
            button(text("Open directory search").size(11))
                .on_press(Message::DirSearchOpen)
                .style(button::secondary),
        );
    }

    col.into()
}

