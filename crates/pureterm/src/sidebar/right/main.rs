use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Background, Border, Color, Element, Length, Shadow, Theme};

use super::data::{BlocksTabData, DirFindData, GitTabData, PortScannerTabData, RemoteFilesTabData, SurfaceFindData, ToolbeltTabData};
use super::view_blocks::view_blocks;
use super::view_feed::view_feed;
use super::view_files::view_files;
use super::view_find::view_find;
use super::view_git::view_git;
use super::view_ports::view_ports;
use super::view_toolbelt::view_toolbelt;
use crate::iced_app::Message;
use crate::sidebar::RightMode;

pub fn view<'a>(
    mode: RightMode,
    width: f32,
    height: f32,
    block_output_enabled: bool,
    surface: Option<SurfaceFindData<'a>>,
    dir: Option<DirFindData<'a>>,
    git: GitTabData<'a>,
    ports: PortScannerTabData<'a>,
    files: RemoteFilesTabData<'a>,
    blocks: BlocksTabData<'a>,
    toolbelt: ToolbeltTabData<'a>,
    #[cfg(unix)] feed_visible: bool,
    #[cfg(unix)] feed_subject: Option<String>,
    #[cfg(unix)] feed_event: Option<String>,
    #[cfg(unix)] feed_tool: Option<String>,
    #[cfg(unix)] feed_reason: Option<String>,
    #[cfg(unix)] feed_remaining_secs: Option<u64>,
    #[cfg(unix)] feed_pending_count: usize,
    #[cfg(unix)] feed_ipc_id: Option<u64>,
) -> Element<'a, Message, Theme, iced::Renderer> {
    let mut tabs = row![].spacing(4);
    for m in RightMode::visible(block_output_enabled) {
        let active = m == mode;
        let label = m.label();
        let msg = Message::SidebarRightMode(m);
        let bg = if active {
            Color::from_rgba(0.35, 0.75, 0.65, 0.25)
        } else {
            Color::TRANSPARENT
        };
        tabs = tabs.push(
            button(text(label).size(11))
                .on_press(msg)
                .padding([4, 8])
                .style(move |_theme: &Theme, _status: button::Status| button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: Color::WHITE,
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        );
    }

    let header = row![
        tabs,
        space().width(Length::Fill),
        button(text("\u{d7}").size(14))
            .on_press(Message::ToggleRightSidebar)
            .padding([2, 6])
            .style(button::text),
    ]
    .spacing(4)
    .padding([6, 8]);

    let content: Element<'a, Message, Theme, iced::Renderer> = match mode {
        RightMode::Find => view_find(surface, dir, width, height),
        #[cfg(unix)]
        RightMode::Feed => view_feed(
            feed_visible,
            feed_subject,
            feed_event,
            feed_tool,
            feed_reason,
            feed_remaining_secs,
            feed_pending_count,
            feed_ipc_id,
        ),
        #[cfg(not(unix))]
        RightMode::Feed => text("Feed (unix only)").size(11).color(dim).into(),
        RightMode::Git => view_git(git),
        RightMode::Ports => view_ports(ports),
        RightMode::Files => view_files(files),
        RightMode::Blocks => view_blocks(blocks),
        RightMode::Toolbelt => view_toolbelt(toolbelt, width),
    };

    let body = scrollable(
        container(content)
            .width(Length::Fill)
            .padding([4, 8]),
    )
    .height(Length::Fill);

    let panel = column![header, body]
        .width(Length::Fixed(width))
        .height(Length::Fixed(height));

    container(panel)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.10, 0.10, 0.12, 1.0))),
            text_color: None,
            snap: false,
            border: Border {
                color: Color::from_rgba(0.25, 0.25, 0.28, 1.0),
                width: 1.0,
                radius: 0.0.into(),
            },
            shadow: Shadow::default(),
        })
        .into()
}

