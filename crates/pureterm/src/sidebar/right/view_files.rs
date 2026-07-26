use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme};

use super::data::{BlocksTabData, DirFindData, GitTabData, PortScannerTabData, RemoteFilesTabData, SurfaceFindData, ToolbeltTabData};
use crate::iced_app::Message;
use crate::sidebar::RightMode;

pub(crate) fn view_files<'a>(files: RemoteFilesTabData<'a>) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = Color::from_rgba(0.55, 0.55, 0.55, 1.0);
    let mut col = column![].spacing(4);

    col = col.push(
        row![
            text("Remote Files").size(12),
            space().width(Length::Fill),
            button(text("↻").size(10))
                .on_press(Message::RemoteFilesRefresh)
                .padding([2, 6])
                .style(button::secondary),
        ]
        .spacing(4),
    );

    if let Some(label) = files.session_label {
        col = col.push(text(label).size(10).color(dim));
    } else {
        col = col.push(text("No daemon session on active pane").size(10).color(dim));
        return col.into();
    }

    col = col.push(text(format!("Path: {}", files.path)).size(10).color(dim));

    col = col.push(
        row![
            text_input("Find name…", files.find_query)
                .on_submit(Message::RemoteFilesFindSubmit)
                .on_input(Message::RemoteFilesFindQueryChanged)
                .size(11)
                .padding(4),
            button(text("Go").size(10))
                .on_press(Message::RemoteFilesFindSubmit)
                .padding([2, 6])
                .style(button::secondary),
        ]
        .spacing(4),
    );
    if files.find_loading {
        col = col.push(text("Searching…").size(10).color(dim));
    } else if let Some(err) = files.find_error {
        col = col.push(text(err).size(10).color(Color::from_rgb(1.0, 0.45, 0.45)));
    } else {
        for m in files.find_results.iter().take(20) {
            let icon = if m.is_dir { "📁" } else { "📄" };
            col = col.push(text(format!("{icon} {}", m.path)).size(9).color(dim));
        }
    }

    if files.can_go_up {
        col = col.push(
            button(text("..").size(10))
                .on_press(Message::RemoteFilesGoUp)
                .width(Length::Fill)
                .style(button::secondary),
        );
    }

    if files.loading {
        col = col.push(text("Loading…").size(10).color(dim));
        return col.into();
    }

    if let Some(err) = files.error {
        col = col.push(text(err).size(10).color(Color::from_rgb(1.0, 0.45, 0.45)));
        return col.into();
    }

    if files.entries.is_empty() {
        col = col.push(text("(empty)").size(10).color(dim));
        return col.into();
    }

    for entry in files.entries {
        let icon = if entry.is_dir { "📁" } else { "📄" };
        let label = format!("{icon} {}", entry.name);
        if entry.is_dir {
            let name = entry.name.clone();
            col = col.push(
                button(text(label).size(10))
                    .on_press(Message::RemoteFilesEnterDir(name))
                    .width(Length::Fill)
                    .style(button::secondary),
            );
        } else {
            let name = entry.name.clone();
            col = col.push(
                button(text(label).size(10))
                    .on_press(Message::RemoteFilesOpenFile(name))
                    .width(Length::Fill)
                    .style(button::secondary),
            );
        }
    }

    if let Some(path) = files.preview_path {
        col = col.push(text("Preview").size(12).color(dim));
        col = col.push(text(path).size(10).color(dim));
        if files.preview_loading {
            col = col.push(text("Loading…").size(10).color(dim));
        } else if let Some(err) = files.preview_error {
            col = col.push(text(err).size(10).color(Color::from_rgb(1.0, 0.45, 0.45)));
        } else if let Some(read) = files.preview {
            if read.truncated {
                col = col.push(
                    text("(truncated preview)".to_string())
                        .size(9)
                        .color(dim),
                );
            }
            let preview: String = read.content.chars().take(4000).collect();
            col = col.push(text(preview).size(9).color(Color::from_rgba(0.85, 0.85, 0.85, 1.0)));
            if files.preview_path.is_some() {
                col = col.push(
                    button(text("Save preview").size(10))
                        .on_press(Message::RemoteFilesSavePreview)
                        .padding([2, 6])
                        .style(button::secondary),
                );
            }
        }
    }

    col.into()
}

