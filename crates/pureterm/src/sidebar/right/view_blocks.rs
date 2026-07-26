use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme};

use super::data::{BlocksTabData, DirFindData, GitTabData, PortScannerTabData, RemoteFilesTabData, SurfaceFindData, ToolbeltTabData};
use crate::iced_app::Message;
use crate::sidebar::RightMode;

pub(crate) fn view_blocks<'a>(blocks: BlocksTabData<'a>) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = Color::from_rgba(0.55, 0.55, 0.55, 1.0);
    let mut col = column![].spacing(4);

    col = col.push(
        row![
            text("Command Blocks").size(12),
            space().width(Length::Fill),
            button(text(blocks.exit_filter.label()).size(10))
                .on_press(Message::BlockOutputCycleFilter)
                .padding([2, 6])
                .style(button::secondary),
        ]
        .spacing(4),
    );

    if !blocks.enabled {
        col = col.push(
            text("Enable [block_output].enabled in config")
                .size(10)
                .color(dim),
        );
        return col.into();
    }

    col = col.push(
        text_input("Search commands & output…", blocks.search)
            .on_input(Message::BlockOutputSearchChanged)
            .size(11)
            .padding(4),
    );

    let q = blocks.search.trim().to_lowercase();
    let filtered: Vec<_> = blocks
        .blocks
        .iter()
        .filter(|b| blocks.exit_filter.matches(b))
        .filter(|b| {
            if q.is_empty() {
                return true;
            }
            b.command
                .as_deref()
                .is_some_and(|c| c.to_lowercase().contains(&q))
                || b
                    .output_text
                    .as_deref()
                    .is_some_and(|t| t.to_lowercase().contains(&q))
        })
        .collect();

    if filtered.is_empty() {
        col = col.push(text("(no blocks yet)").size(10).color(dim));
        return col.into();
    }

    for block in filtered.iter().rev().take(64) {
        let cmd = block
            .command
            .as_deref()
            .unwrap_or("(shell)");
        let exit = block
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into());
        let dur = block
            .duration_ms()
            .map(|ms| format!("{ms}ms"))
            .unwrap_or_default();
        let exit_color = if block.failed() {
            Color::from_rgb(1.0, 0.45, 0.45)
        } else {
            Color::from_rgba(0.55, 0.85, 0.65, 1.0)
        };
        let label = format!("exit {exit}  {dur}");
        col = col.push(
            button(
                column![
                    text(cmd).size(10),
                    text(label).size(9).color(exit_color),
                    if let Some(out) = block.output_text.as_deref() {
                        let preview: String = out.chars().take(120).collect();
                        text(preview).size(8).color(dim)
                    } else {
                        text("").size(8)
                    },
                ]
                .spacing(2),
            )
            .on_press(Message::BlockOutputJumpToRow(block.command_row))
            .width(Length::Fill)
            .style(button::secondary),
        );
    }

    col.into()
}

