use iced::widget::{column, text};
use iced::{Color, Element, Theme};

use super::data::ToolbeltTabData;
use pureterm_settings::theme::palette::ShellTokens;

use crate::iced_app::Message;

pub(crate) fn view_toolbelt<'a>(
    tokens: ShellTokens,
    data: ToolbeltTabData<'a>,
    _width: f32,
) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = tokens.text_secondary;
    let mut col = column![].spacing(6);

    col = col.push(text("Command history").size(12).color(dim));
    if data.history.is_empty() {
        col = col.push(text("(empty — run commands in a pane)").size(10).color(dim));
    } else {
        for line in data.history.iter().rev().take(20) {
            col = col.push(text(line).size(10));
        }
    }

    col = col.push(text("Bookmarks").size(12).color(dim));
    for b in data.bookmarks {
        col = col.push(text(format!("{} → {}", b.name, b.path)).size(10));
    }

    col = col.push(text("Snippets").size(12).color(dim));
    for s in data.snippets {
        col = col.push(text(format!("{}: {}", s.name, s.text)).size(10));
    }

    col = col.push(text("Paste history").size(12).color(dim));
    for p in data.paste_history.iter().rev().take(10) {
        let preview: String = p.chars().take(60).collect();
        col = col.push(text(preview).size(10));
    }

    col.into()
}

