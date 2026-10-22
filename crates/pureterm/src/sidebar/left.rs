//! GAP-19:左侧边栏——工作区列表 + Git 分支摘要。

use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Background, Border, Color, Element, Length, Shadow, Theme};

use pureterm_settings::theme::palette::ShellTokens;

use crate::iced_app::{Message, WorkspaceId};

/// 左侧边栏中一个工作区行的快照。
pub struct WorkspaceRow {
    pub id: WorkspaceId,
    pub name: String,
    pub active: bool,
    pub tab_count: usize,
}

/// 供左侧边栏底部使用的 Git 分支摘要。
pub struct GitSummary<'a> {
    pub branch: Option<&'a str>,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
    pub dirty: bool,
}

pub fn view<'a>(
    tokens: ShellTokens,
    workspaces: &[WorkspaceRow],
    git: GitSummary<'a>,
    dsl_items: &[(String, String)],
    width: f32,
    height: f32,
) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = tokens.text_secondary;
    let accent = tokens.accent;

    let mut list = column![].spacing(2).padding([4, 6]);
    list = list.push(text("Workspaces").size(12).color(dim));
    list = list.push(space().height(4));

    if workspaces.is_empty() {
        list = list.push(text("(none)").size(11).color(dim));
    } else {
        for ws in workspaces {
            let label = format!("{}  ({})", ws.name, ws.tab_count);
            let bg = if ws.active {
                tokens.highlight_faint
            } else {
                Color::TRANSPARENT
            };
            let id = ws.id;
            let active = ws.active;
            let btn = button(text(label).size(11))
                .on_press(Message::ActivateWorkspaceById(id))
                .width(Length::Fill)
                .padding([4, 6])
                .style(move |_theme: &Theme, _status: button::Status| button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: if active { accent } else { tokens.text_primary },
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            list = list.push(btn);
        }
    }

    list = list.push(space().height(8));
    list = list.push(text("Git").size(12).color(dim));
    list = list.push(space().height(2));

    if let Some(branch) = git.branch {
        let mut line = format!("\u{e0a0} {branch}");
        if let Some(a) = git.ahead {
            if a > 0 {
                line.push_str(&format!("  \u{2191}{a}"));
            }
        }
        if let Some(b) = git.behind {
            if b > 0 {
                line.push_str(&format!("  \u{2193}{b}"));
            }
        }
        if git.dirty {
            line.push_str("  *");
        }
        list = list.push(text(line).size(11));
    } else {
        list = list.push(text("No repo").size(11).color(dim));
    }

    if !dsl_items.is_empty() {
        list = list.push(space().height(8));
        list = list.push(text("Events").size(12).color(dim));
        list = list.push(space().height(2));
        for (icon, line) in dsl_items {
            list = list.push(text(format!("{icon} {line}")).size(11));
        }
    }

    let header = row![
        text("Sidebar").size(13),
        space().width(Length::Fill),
        button(text("\u{d7}").size(14))
            .on_press(Message::ToggleLeftSidebar)
            .padding([2, 6])
            .style(button::text),
    ]
    .spacing(4)
    .padding([6, 8]);

    let body = scrollable(list).height(Length::Fill);

    let panel = column![header, body]
        .width(Length::Fixed(width))
        .height(Length::Fixed(height));

    let panel_bg = tokens.surface;
    let panel_border = tokens.hairline;
    container(panel)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(Background::Color(panel_bg)),
            text_color: None,
            snap: false,
            border: Border {
                color: panel_border,
                width: 1.0,
                radius: 0.0.into(),
            },
            shadow: Shadow::default(),
        })
        .into()
}
