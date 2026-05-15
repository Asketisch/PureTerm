//! Customizable status bar with dynamic components.
//!
//! Modeled on iTerm2's `sources/StatusBar/`. This module is pure data + lookup;
//! the egui pane rendering surface is a follow-up.
//!
//! A [`StatusBar`] holds an ordered list of [`StatusComponent`] values and
//! renders them against a [`StatusContext`] snapshot built each frame by the
//! pane.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Placeholder rendered when a component has no value.
const MISSING: &str = "—";

/// One configurable element in the status bar.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum StatusComponent {
    /// Current working directory.
    Cwd,
    /// Hostname of the machine the session is on.
    Hostname,
    /// Login user.
    User,
    /// Git branch resolved from `repo_root` (if `None`, resolved from cwd).
    GitBranch { repo_root: Option<String> },
    /// Foreground job name (e.g. the running command).
    JobName,
    /// Wall clock formatted via `chrono::Local::now().format(fmt)`.
    Clock { fmt: String },
    /// User-defined component. Rendering only emits `[label]`; background
    /// refresh of `command` is a follow-up.
    Custom {
        label: String,
        command: String,
        refresh_secs: u32,
    },
}

/// Snapshot of values used to render a status bar. The pane builds this each
/// frame from its session/terminal state.
#[derive(Default, Debug, Clone)]
pub struct StatusContext {
    pub cwd: Option<String>,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub git_branch: Option<String>,
    pub job_name: Option<String>,
}

/// Ordered list of components plus an enable flag.
#[derive(Clone, Debug)]
pub struct StatusBar {
    pub components: Vec<StatusComponent>,
    pub enabled: bool,
}

impl StatusBar {
    pub fn new(components: Vec<StatusComponent>) -> Self {
        Self {
            components,
            enabled: true,
        }
    }

    /// Default layout: cwd, git branch, clock (HH:MM).
    pub fn default_layout() -> Self {
        Self::new(vec![
            StatusComponent::Cwd,
            StatusComponent::GitBranch { repo_root: None },
            StatusComponent::Clock {
                fmt: "%H:%M".to_string(),
            },
        ])
    }

    /// Render every component into a string. Missing values become [`MISSING`].
    /// The result has the same length as `self.components`.
    pub fn render(&self, ctx: &StatusContext) -> Vec<String> {
        self.components
            .iter()
            .map(|c| render_component(c, ctx))
            .collect()
    }
}

fn render_component(component: &StatusComponent, ctx: &StatusContext) -> String {
    match component {
        StatusComponent::Cwd => ctx.cwd.clone().unwrap_or_else(|| MISSING.to_string()),
        StatusComponent::Hostname => ctx
            .hostname
            .clone()
            .unwrap_or_else(|| MISSING.to_string()),
        StatusComponent::User => ctx.user.clone().unwrap_or_else(|| MISSING.to_string()),
        StatusComponent::GitBranch { repo_root } => {
            // Prefer the precomputed value from the context; fall back to a
            // synchronous lookup against the configured root, then cwd.
            if let Some(branch) = ctx.git_branch.clone() {
                return branch;
            }
            let root_str = repo_root.clone().or_else(|| ctx.cwd.clone());
            root_str
                .as_deref()
                .map(Path::new)
                .and_then(read_git_branch)
                .unwrap_or_else(|| MISSING.to_string())
        }
        StatusComponent::JobName => ctx
            .job_name
            .clone()
            .unwrap_or_else(|| MISSING.to_string()),
        StatusComponent::Clock { fmt } => chrono::Local::now().format(fmt).to_string(),
        StatusComponent::Custom { label, .. } => format!("[{}]", label),
    }
}

/// Cheap synchronous git branch lookup. Walks up from `start` looking for a
/// `.git/HEAD`; if found, parses `ref: refs/heads/<name>` and returns the
/// branch name. If HEAD points directly at a 40-char SHA (detached), returns
/// the first 7 chars.
pub fn read_git_branch(start: &Path) -> Option<String> {
    let head_path = find_git_head(start)?;
    let raw = std::fs::read_to_string(&head_path).ok()?;
    parse_head(&raw)
}

fn find_git_head(start: &Path) -> Option<PathBuf> {
    // Try the path as-is, then walk parents.
    let mut current: Option<&Path> = Some(start);
    while let Some(dir) = current {
        let candidate = dir.join(".git").join("HEAD");
        if candidate.is_file() {
            return Some(candidate);
        }
        // Also handle the case where `start` itself IS a `.git` dir.
        if dir.file_name().and_then(|n| n.to_str()) == Some(".git") {
            let candidate = dir.join("HEAD");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        current = dir.parent();
    }
    None
}

fn parse_head(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("ref:") {
        let r = rest.trim();
        // refs/heads/<name>
        let name = r.rsplit('/').next().unwrap_or(r);
        if name.is_empty() {
            return None;
        }
        return Some(name.to_string());
    }
    // Detached: expect a 40-char hex SHA.
    if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(trimmed.chars().take(7).collect());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn default_layout_has_three_components() {
        let bar = StatusBar::default_layout();
        assert_eq!(bar.components.len(), 3);
        assert!(bar.enabled);
        assert_eq!(bar.components[0], StatusComponent::Cwd);
        assert_eq!(
            bar.components[1],
            StatusComponent::GitBranch { repo_root: None }
        );
        assert!(matches!(bar.components[2], StatusComponent::Clock { .. }));
    }

    #[test]
    fn render_uses_dashes_for_missing_values() {
        let bar = StatusBar::new(vec![
            StatusComponent::Cwd,
            StatusComponent::Hostname,
            StatusComponent::User,
            StatusComponent::JobName,
            StatusComponent::GitBranch { repo_root: None },
        ]);
        let out = bar.render(&StatusContext::default());
        assert_eq!(out.len(), 5);
        for s in out {
            assert_eq!(s, MISSING);
        }
    }

    #[test]
    fn render_clock_returns_string() {
        let bar = StatusBar::new(vec![StatusComponent::Clock {
            fmt: "%H:%M".to_string(),
        }]);
        let out = bar.render(&StatusContext::default());
        assert_eq!(out.len(), 1);
        assert!(!out[0].is_empty(), "clock string should be non-empty");
    }

    #[test]
    fn render_custom_uses_label_placeholder() {
        let bar = StatusBar::new(vec![StatusComponent::Custom {
            label: "k8s".to_string(),
            command: "kubectl config current-context".to_string(),
            refresh_secs: 30,
        }]);
        let out = bar.render(&StatusContext::default());
        assert_eq!(out, vec!["[k8s]".to_string()]);
    }

    #[test]
    fn render_uses_context_values_when_present() {
        let bar = StatusBar::new(vec![
            StatusComponent::Cwd,
            StatusComponent::Hostname,
            StatusComponent::GitBranch { repo_root: None },
        ]);
        let ctx = StatusContext {
            cwd: Some("/tmp/x".to_string()),
            hostname: Some("h1".to_string()),
            git_branch: Some("main".to_string()),
            ..StatusContext::default()
        };
        let out = bar.render(&ctx);
        assert_eq!(out, vec!["/tmp/x", "h1", "main"]);
    }

    #[test]
    fn git_branch_reads_ref_head() {
        let dir = TempDir::new().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir(&git).unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
        assert_eq!(read_git_branch(dir.path()), Some("feature".to_string()));
    }

    #[test]
    fn git_branch_returns_short_sha_for_detached() {
        let dir = TempDir::new().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir(&git).unwrap();
        let sha = "abcdef0123456789abcdef0123456789abcdef01";
        fs::write(git.join("HEAD"), format!("{sha}\n")).unwrap();
        assert_eq!(read_git_branch(dir.path()), Some("abcdef0".to_string()));
    }

    #[test]
    fn git_branch_returns_none_when_no_git() {
        let dir = TempDir::new().unwrap();
        assert_eq!(read_git_branch(dir.path()), None);
    }

    #[test]
    fn git_branch_walks_up_from_subdirectory() {
        let dir = TempDir::new().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir(&git).unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/dev\n").unwrap();
        let sub = dir.path().join("a").join("b");
        fs::create_dir_all(&sub).unwrap();
        assert_eq!(read_git_branch(&sub), Some("dev".to_string()));
    }
}
