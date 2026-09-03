#[cfg(unix)]
use iced::widget::{button, column, row, space, text};
use iced::{Alignment, Color, Element, Length, Theme};

use crate::iced_app::Message;

pub(crate) fn view_feed(
    visible: bool,
    subject: Option<String>,
    event: Option<String>,
    tool: Option<String>,
    reason: Option<String>,
    remaining_secs: Option<u64>,
    pending_count: usize,
    ipc_id: Option<u64>,
) -> Element<'static, Message, Theme, iced::Renderer> {
    use crate::feed_client::FeedChoice;

    let dim = Color::from_rgba(0.55, 0.55, 0.55, 1.0);

    if !visible {
        return column![
            text("No pending Feed approvals").size(11).color(dim),
            text("Hook requests appear here.").size(10).color(dim),
        ]
        .spacing(4)
        .into();
    }

    let id = ipc_id.unwrap_or(0);
    let subject_line = subject.as_deref().unwrap_or("(no preview)");
    let truncated: String = subject_line.chars().take(400).collect();

    let choice_btn = |label: &'static str, choice: FeedChoice| {
        button(text(label).size(11))
            .on_press(Message::FeedResolve { id, choice })
            .width(Length::Fill)
            .padding([4, 6])
            .style(button::secondary)
    };

    let mut col = column![
        text("Feed Approval").size(12),
        text(format!("事件: {}", event.as_deref().unwrap_or("-"))).size(10),
        text(format!("工具: {}", tool.as_deref().unwrap_or("-"))).size(10),
        text(format!(
            "理由: {}",
            reason
                .as_deref()
                .filter(|r| !r.is_empty())
                .unwrap_or("-")
        ))
        .size(10),
        text("内容:").size(10),
        text(truncated).size(10),
        choice_btn("Once", FeedChoice::Once),
        choice_btn("Always", FeedChoice::Always),
        choice_btn("All", FeedChoice::All),
        choice_btn("Bypass", FeedChoice::Bypass),
        choice_btn("Deny", FeedChoice::Deny),
    ]
    .spacing(4);

    if let Some(secs) = remaining_secs {
        col = col.push(text(format!("⏱ auto Deny: {secs}s")).size(10).color(dim));
    }
    if pending_count > 1 {
        col = col.push(
            row![
                text(format!("+{} pending", pending_count - 1)).size(10).color(dim),
                space().width(Length::Fill),
                button(text("Next →").size(10))
                    .on_press(Message::FeedFocusNext)
                    .style(button::secondary),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }

    col.into()
}

