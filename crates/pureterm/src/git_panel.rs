//! GAP-32: Git overlay panel — a read-only view of the focused pane's repo
//! (branch / upstream / ahead-behind / per-category counts / recent commits /
//! in-progress op / linked PR) plus quick-action buttons that write git
//! commands into the pane's PTY.
//!
//! This module owns the panel *state* + the [`GitQuickAction`] enum. The view
//! (`App::view_git_panel`) and the `Message` wiring live in `iced_app.rs`, which
//! has the app's private fields (active pane, terminal contexts, config) in
//! scope. State is repurposed (not recreated) when the user switches panes while
//! the panel is open, so a power user can keep it pinned across repos.

use crate::gh_pr::PrInfo;
use crate::status_bar::git::GitStatus;

/// A quick git action fired from the panel's button row. Each maps to a command
/// written into the focused pane's PTY (see `iced_app::Message::GitQuickAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitQuickAction {
    /// `git add -A`
    StageAll,
    /// `git commit` (opens the editor in-pane)
    Commit,
    /// `git push`
    Push,
    /// `git pull`
    Pull,
    /// `lazygit` (or the configured binary) — opens the TUI in-pane.
    Lazygit,
}

/// State for the Git overlay panel.
#[derive(Debug, Clone, Default)]
pub struct GitPanelState {
    /// Panel visibility.
    pub open: bool,
    /// The cwd this panel is currently showing (staleness key for async results).
    pub cwd: Option<String>,
    /// Last known enriched git status for `cwd` (shared shape with the status bar).
    pub status: Option<GitStatus>,
    /// Last known linked PR for `cwd`.
    pub pr: Option<PrInfo>,
    /// Recent commits (`(short_sha, subject)`) for `cwd`, fetched on open/refresh.
    pub last_log: Vec<(String, String)>,
    /// True while a `gh pr view` refresh is in flight.
    pub pr_refreshing: bool,
    /// Banner shown in the PR block when `gh` is missing/unauthed/etc.
    pub pr_error: Option<String>,
    /// Counts status-bar ticks to throttle `gh` refreshes (gh is slow/networked).
    pub pr_tick_counter: u64,
}

impl GitPanelState {
    /// Fresh closed panel (used at app startup).
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the panel for a given cwd. Does not presume a PR refresh is in
    /// flight — callers set `pr_refreshing` only when they actually fire a
    /// `gh` task (gated by `[git].show_pr`), so a disabled PR query never leaves
    /// the panel stuck on "refreshing…".
    pub fn open_for(&mut self, cwd: &str) {
        self.open = true;
        self.cwd = Some(cwd.to_string());
        self.status = None;
        self.pr = None;
        self.last_log = Vec::new();
        self.pr_refreshing = false;
        self.pr_error = None;
        self.pr_tick_counter = 0;
    }

    /// Close the panel (keeps `cwd` for a potential re-open, clears live state).
    pub fn close(&mut self) {
        self.open = false;
        self.pr_refreshing = false;
    }
}
