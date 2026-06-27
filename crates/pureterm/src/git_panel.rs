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
    /// `gh pr create` (opens the editor in-pane for title/body). Needs `gh`
    /// installed + authed; the caller toasts if `gh` is missing.
    CreatePr,
    /// `lazygit` (or the configured binary) — opens the TUI in-pane.
    Lazygit,
}

impl GitQuickAction {
    /// Lowercase slug used in the `git.quick_action` event payload (GAP-32
    /// EventBus). Stable wire name — do not rename without a migration.
    pub fn as_slug(self) -> &'static str {
        match self {
            GitQuickAction::StageAll => "stage_all",
            GitQuickAction::Commit => "commit",
            GitQuickAction::Push => "push",
            GitQuickAction::Pull => "pull",
            GitQuickAction::CreatePr => "create_pr",
            GitQuickAction::Lazygit => "lazygit",
        }
    }
}

/// GAP-32 EventBus: did the git branch change enough to emit `git.branch_changed`?
/// `None` prev (first observation for a pane) counts as a change so the initial
/// branch is recorded; subsequent identical observations are suppressed (the
/// status path refreshes git every tick — this keeps `events.jsonl` quiet).
/// Pure + tested.
pub fn git_branch_changed(prev: Option<&str>, next: &str) -> bool {
    match prev {
        Some(p) => p != next,
        None => true,
    }
}

/// Pure mapping from a quick action to the command string written into the
/// focused pane's PTY (GAP-32). Extracted from `run_git_quick_action` so the
/// command table is unit-testable without app state. The side-effecting guards
/// — lazygit on an SSH pane / a missing `lazygit` or `gh` binary, and closing
/// the panel for lazygit's TUI — stay in the caller, since they need app state
/// and toasts. `Lazygit` resolves the configured binary name from `[git]`.
pub fn git_quick_action_command(
    action: GitQuickAction,
    cfg: &crate::app_config::GitConfig,
) -> Option<String> {
    match action {
        GitQuickAction::StageAll => Some("git add -A".into()),
        GitQuickAction::Commit => Some("git commit".into()),
        GitQuickAction::Push => Some("git push".into()),
        GitQuickAction::Pull => Some("git pull".into()),
        GitQuickAction::CreatePr => Some("gh pr create".into()),
        GitQuickAction::Lazygit => Some(
            cfg.lazygit_cmd
                .clone()
                .unwrap_or_else(|| "lazygit".to_string()),
        ),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::GitConfig;

    #[test]
    fn quick_action_command_table() {
        let cfg = GitConfig::default();
        assert_eq!(
            git_quick_action_command(GitQuickAction::StageAll, &cfg).as_deref(),
            Some("git add -A")
        );
        assert_eq!(
            git_quick_action_command(GitQuickAction::Commit, &cfg).as_deref(),
            Some("git commit")
        );
        assert_eq!(
            git_quick_action_command(GitQuickAction::Push, &cfg).as_deref(),
            Some("git push")
        );
        assert_eq!(
            git_quick_action_command(GitQuickAction::Pull, &cfg).as_deref(),
            Some("git pull")
        );
        // GAP-32: Create-PR writes `gh pr create` into the pane.
        assert_eq!(
            git_quick_action_command(GitQuickAction::CreatePr, &cfg).as_deref(),
            Some("gh pr create")
        );
        assert_eq!(
            git_quick_action_command(GitQuickAction::Lazygit, &cfg).as_deref(),
            Some("lazygit")
        );
    }

    #[test]
    fn quick_action_lazygit_honors_config_override() {
        let mut cfg = GitConfig::default();
        cfg.lazygit_cmd = Some("lazygit-nightly".into());
        assert_eq!(
            git_quick_action_command(GitQuickAction::Lazygit, &cfg).as_deref(),
            Some("lazygit-nightly")
        );
    }

    #[test]
    fn quick_action_slugs_are_stable() {
        // Wire names emitted into events.jsonl — must not drift.
        assert_eq!(GitQuickAction::CreatePr.as_slug(), "create_pr");
        assert_eq!(GitQuickAction::StageAll.as_slug(), "stage_all");
        assert_eq!(GitQuickAction::Lazygit.as_slug(), "lazygit");
    }

    #[test]
    fn branch_changed_dedup() {
        // First observation for a pane → emit.
        assert!(git_branch_changed(None, "main"));
        // Same branch → suppress.
        assert!(!git_branch_changed(Some("main"), "main"));
        // Different branch → emit.
        assert!(git_branch_changed(Some("main"), "feature/x"));
        // Detached short-SHA vs branch → emit.
        assert!(git_branch_changed(Some("a1b2c3d"), "main"));
    }
}
