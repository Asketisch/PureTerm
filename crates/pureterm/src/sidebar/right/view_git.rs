use iced::widget::{button, column, row, space, text};
use iced::{Color, Element, Length, Theme};

use super::data::GitTabData;
use crate::iced_app::Message;
use crate::git_panel::GitQuickAction;
use crate::status_bar::GitOp;

pub(crate) fn view_git<'a>(git: GitTabData<'a>) -> Element<'a, Message, Theme, iced::Renderer> {
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
