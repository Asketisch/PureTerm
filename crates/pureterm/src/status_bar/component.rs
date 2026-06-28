//! GAP-24: the status-bar component trait + the per-frame data snapshot.
//!
//! Each segment of the bottom bar is a `StatusBarComponent`. The app builds a
//! `BarData` snapshot from its live state inside `view_bottom_bar` (where all
//! private fields/methods are in scope) and passes `&BarData` to each
//! component's `view`. Components hold their own stat state (CPU/Memory/Network
//! ring buffers, etc.), updated via `apply_stats`/`apply_git`.
//!
//! Design notes:
//! - `view` returns `BarElement<'static>` (an owned `iced::Element`). Widgets are
//!   built from owned strings / cloned samples, so no borrow entangles the
//!   returned element with `&self` or `&BarData` — this sidesteps the lifetime
//!   gymnastics a `view<'a>(&'a self, ...)` would force on the registry.
//! - `BarData` deliberately carries only portable types (no cfg-gated SSH/Vi
//!   enums): the cfg-gated formatting stays in `view_bottom_bar`, which
//!   pre-formats SSH/Vi into `(String, Color)` / `&'static str`.
//! - `Send` because `PureTermApp` must be `Send` for the tokio executor and the
//!   registry is a field of it. (The app is intentionally not `Sync`.)

use iced::{Color, Element};

use crate::iced_app::Message;
use crate::notification_store::NotificationSummary;

use super::git::GitStatus;
use super::stats::StatSnapshot;

/// Owned status-bar element bound to the app's `Message`/`Theme`/`Renderer`.
pub(crate) type BarElement<'a> = Element<'a, Message, iced::Theme, iced::Renderer>;

/// Read-only snapshot of the live app state a component may render.
///
/// All per-pane / per-frame values (cwd, git, ssh, vi, process, agent, cursor)
/// are pre-computed by `view_bottom_bar`. System metrics live inside the
/// stat-holding components themselves, not here.
pub struct BarData<'a> {
    pub cwd: Option<&'a str>,
    /// Instant pure-filesystem branch read (always fresh on cwd/pane change).
    pub git_branch: Option<&'a str>,
    /// Async porcelain-v2 upgrade (None until the first `StatusBarTick` resolves).
    pub git_status: Option<&'a GitStatus>,
    /// Pre-formatted SSH segment `("  SSH …", color)`, if applicable.
    pub ssh: Option<(String, Color)>,
    /// Pre-formatted vi/copy-mode label, if vi mode is active.
    pub vi_mode: Option<&'static str>,
    pub process: Option<&'a str>,
    /// Pre-formatted agent indicator `("⚡", color)`, if applicable.
    pub agent: Option<(String, Color)>,
    /// GAP-09: unread-notification summary (status-bar count). `None`/zero ⇒ hide.
    pub notifications: Option<NotificationSummary>,
    pub cursor: (usize, usize),
    pub show_sparklines: bool,
}

/// One segment of the bottom status bar.
pub trait StatusBarComponent: Send {
    /// Stable component id (matches `[statusbar].components` entries).
    fn id(&self) -> &'static str;
    /// True ⇒ render right of the flexible spacer (e.g. clock, cursor).
    fn align_right(&self) -> bool {
        false
    }
    /// Render the segment as an owned element.
    fn view(&self, data: &BarData<'_>) -> BarElement<'static>;
    /// Receive the latest system-stat snapshot (default no-op).
    fn apply_stats(&mut self, _snap: &StatSnapshot) {}
    /// Receive the latest enriched git status (default no-op).
    fn apply_git(&mut self, _git: &GitStatus) {}
}
