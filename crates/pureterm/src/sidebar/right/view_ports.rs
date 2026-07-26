use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme};

use super::data::{BlocksTabData, DirFindData, GitTabData, PortScannerTabData, RemoteFilesTabData, SurfaceFindData, ToolbeltTabData};
use crate::iced_app::Message;
use crate::sidebar::RightMode;

pub(crate) fn view_ports<'a>(ports: PortScannerTabData<'a>) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = Color::from_rgba(0.55, 0.55, 0.55, 1.0);
    let mut col = column![].spacing(4);

    col = col.push(
        row![
            text("Listening Ports").size(12),
            space().width(Length::Fill),
            if ports.scanning {
                text("…").size(10).color(dim)
            } else {
                text(format!("{}", ports.entries.len()))
                    .size(10)
                    .color(dim)
            },
        ]
        .spacing(4),
    );

    if ports.entries.is_empty() {
        col = col.push(text("No listening ports (≥ min_port)").size(10).color(dim));
        return col.into();
    }

    for entry in ports.entries {
        let label = format!(
            "{}  {}  pid {}",
            entry.port, entry.process, entry.pid
        );
        col = col.push(
            row![
                button(text(label).size(10))
                    .on_press(Message::PortScannerCopy(entry.port))
                    .width(Length::Fill),
                button(text("↗").size(10))
                    .on_press(Message::PortScannerOpen(entry.port))
                    .padding([2, 6])
                    .style(button::secondary),
            ]
            .spacing(2),
        );
    }

    col = col.push(
        text("Click row → copy localhost:PORT; ↗ opens browser")
            .size(9)
            .color(dim),
    );

    col.into()
}

