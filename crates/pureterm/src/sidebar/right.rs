//! GAP-19: Right sidebar — Find / Feed / Git mode tabs.

use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme};

use crate::git_panel::GitQuickAction;
use crate::iced_app::Message;
use crate::sidebar::RightMode;
use crate::status_bar::GitOp;

/// Git panel data for the Git tab (mirrors overlay fields).
pub struct GitTabData<'a> {
    pub status: Option<&'a crate::status_bar::git::GitStatus>,
    pub pr: Option<&'a crate::gh_pr::PrInfo>,
    pub pr_refreshing: bool,
    pub pr_error: Option<&'a str>,
    pub last_log: &'a [(String, String)],
    pub cwd: Option<&'a str>,
    pub show_pr: bool,
}

/// Surface search fields for the Find tab.
pub struct SurfaceFindData<'a> {
    pub query: &'a str,
    pub case_sensitive: bool,
    pub regex_mode: bool,
    pub whole_word: bool,
    pub follow_tail: bool,
}

/// Directory search fields for the Find tab.
pub struct DirFindData<'a> {
    pub query: &'a str,
    pub running: bool,
    pub error: Option<&'a str>,
    pub results: &'a [crate::search::RgMatch],
}

/// Toolbelt tab data (GAP-30).
pub struct ToolbeltTabData<'a> {
    pub history: &'a [String],
    pub bookmarks: &'a [crate::toolbelt::ToolbeltBookmark],
    pub snippets: &'a [crate::toolbelt::ToolbeltSnippet],
    pub paste_history: &'a [String],
}

/// Port scanner rows for the Ports tab (GAP-33).
pub struct PortScannerTabData<'a> {
    pub entries: &'a [crate::port_scanner::ListeningPort],
    pub scanning: bool,
}

/// Remote directory listing for the Files tab (GAP-43).
pub struct RemoteFilesTabData<'a> {
    pub path: &'a str,
    pub entries: &'a [crate::daemon_client::RemoteFileEntry],
    pub loading: bool,
    pub error: Option<&'a str>,
    pub session_label: Option<&'a str>,
    pub can_go_up: bool,
    pub preview_path: Option<&'a str>,
    pub preview_loading: bool,
    pub preview: Option<&'a crate::daemon_client::RemoteFileRead>,
    pub preview_error: Option<&'a str>,
    pub find_query: &'a str,
    pub find_results: &'a [crate::daemon_client::RemoteFindMatch],
    pub find_loading: bool,
    pub find_error: Option<&'a str>,
}

/// Block list for the Blocks tab (GAP-51).
pub struct BlocksTabData<'a> {
    pub blocks: &'a [crate::output_blocks::OutputBlock],
    pub exit_filter: crate::output_blocks::BlockExitFilter,
    pub search: &'a str,
    pub enabled: bool,
}

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

fn view_find<'a>(
    surface: Option<SurfaceFindData<'a>>,
    dir: Option<DirFindData<'a>>,
    _width: f32,
    _height: f32,
) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = Color::from_rgba(0.55, 0.55, 0.55, 1.0);
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
            col = col.push(text(err).size(11).color(Color::from_rgb(0.9, 0.5, 0.4)));
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

#[cfg(unix)]
fn view_feed(
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

fn view_toolbelt<'a>(
    data: ToolbeltTabData<'a>,
    _width: f32,
) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = Color::from_rgba(0.55, 0.55, 0.55, 1.0);
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

fn view_files<'a>(files: RemoteFilesTabData<'a>) -> Element<'a, Message, Theme, iced::Renderer> {
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

fn view_blocks<'a>(blocks: BlocksTabData<'a>) -> Element<'a, Message, Theme, iced::Renderer> {
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

fn view_ports<'a>(ports: PortScannerTabData<'a>) -> Element<'a, Message, Theme, iced::Renderer> {
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

fn view_git<'a>(git: GitTabData<'a>) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = Color::from_rgba(0.55, 0.55, 0.55, 1.0);
    let amber = Color::from_rgba(0.95, 0.75, 0.3, 1.0);
    let mut col = column![].spacing(5);

    col = col.push(
        row![
            text("Git").size(12),
            space().width(Length::Fill),
            button(text("\u{21bb}").size(11))
                .on_press(Message::RefreshGitPanel)
                .padding([2, 6])
                .style(button::secondary),
        ]
        .spacing(4),
    );

    if let Some(g) = git.status {
        let branch = g.branch.clone().unwrap_or_else(|| "(unknown)".to_string());
        col = col.push(text(format!("\u{e0a0} {branch}")).size(11));
        if let Some(u) = &g.upstream {
            let mut line = format!("  \u{2194} {u}");
            if let Some(a) = g.ahead {
                if a > 0 {
                    line.push_str(&format!("  \u{2191}{a}"));
                }
            }
            if let Some(b) = g.behind {
                if b > 0 {
                    line.push_str(&format!("  \u{2193}{b}"));
                }
            }
            col = col.push(text(line).size(10).color(dim));
        }
        col = col.push(text(format!(
            "  ●{} ~{} +{} !{}",
            g.staged, g.unstaged, g.untracked, g.unmerged,
        ))
        .size(10));
        if g.op != GitOp::None {
            col = col.push(
                text(format!("\u{21bb} {} in progress", g.op.as_str()))
                    .size(10)
                    .color(amber),
            );
        }
    } else {
        col = col.push(text("Resolving repo…").size(10).color(dim));
    }

    if !git.last_log.is_empty() {
        col = col.push(text("Recent commits").size(10).color(dim));
        for (h, s) in git.last_log {
            col = col.push(text(format!("{h} {s}")).size(9));
        }
    }

    if git.show_pr {
        if let Some(pr) = git.pr {
            col = col.push(
                text(format!("#{} {}", pr.number, pr.state))
                    .size(10),
            );
        } else if git.pr_refreshing {
            col = col.push(text("PR: refreshing…").size(10).color(dim));
        } else if let Some(err) = git.pr_error {
            col = col.push(text(format!("PR: {err}")).size(10).color(dim));
        }
    }

    col = col.push(
        row![
            button(text("Stage").size(10))
                .on_press(Message::GitQuickAction(GitQuickAction::StageAll))
                .style(button::secondary),
            button(text("Commit").size(10))
                .on_press(Message::GitQuickAction(GitQuickAction::Commit))
                .style(button::secondary),
            button(text("Push").size(10))
                .on_press(Message::GitQuickAction(GitQuickAction::Push))
                .style(button::secondary),
            button(text("Pull").size(10))
                .on_press(Message::GitQuickAction(GitQuickAction::Pull))
                .style(button::secondary),
        ]
        .spacing(4),
    );
    col = col.push(
        button(text("Lazygit").size(10))
            .on_press(Message::GitQuickAction(GitQuickAction::Lazygit))
            .style(button::secondary),
    );

    if let Some(cwd) = git.cwd {
        let short: String = if cwd.len() > 28 {
            cwd.chars().skip(cwd.len() - 28).collect()
        } else {
            cwd.to_string()
        };
        col = col.push(text(short).size(9).color(dim));
    }

    col.into()
}
