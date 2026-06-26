//! GAP-24: Git status for the status bar.
//!
//! Two layers:
//! - `read_git_branch` — cheap synchronous pure-filesystem branch lookup (no
//!   `git` subprocess). Kept verbatim from the legacy `status_bar.rs` so the
//!   instant OSC7 / pane-switch path never regresses and so we have a fallback
//!   when `git` is not installed.
//! - `spawn_git_status` — async porcelain-v2 upgrade: branch + dirty +
//!   ahead/behind in one subprocess, run off the GUI thread via `Task::perform`.
//!
//! `parse_porcelain_v2` is a pure, unit-tested function.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

/// Enriched Git status for the status bar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub dirty: bool,
    /// Commits ahead of upstream (`None` when there is no upstream).
    pub ahead: Option<usize>,
    /// Commits behind upstream.
    pub behind: Option<usize>,
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

/// Spawn the async git-status upgrade. Runs `git status --porcelain=v2 --branch`
/// off the GUI thread; maps the result back to [`Message::GitStatusResult`].
/// Falls back to the pure-FS branch read if `git` is missing or the dir is not a
/// repo, so behavior never drops below today's status bar.
pub fn spawn_git_status(cwd: String) -> iced::Task<crate::iced_app::Message> {
    iced::Task::perform(
        async move {
            let out = Command::new("git")
                .args(["-C", &cwd, "status", "--porcelain=v2", "--branch"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .await;
            match out {
                Ok(o) if o.status.success() => {
                    let mut gs = parse_porcelain_v2(&String::from_utf8_lossy(&o.stdout));
                    // Detached HEAD: porcelain emits "# branch.head (detached)";
                    // resolve the short SHA so the bar shows something useful.
                    if gs.branch.as_deref() == Some("(detached)") {
                        gs.branch = short_sha(&cwd)
                            .await
                            .or_else(|| read_git_branch(Path::new(&cwd)));
                    }
                    (cwd, gs)
                }
                // git not installed / not a repo / non-zero exit → pure-FS fallback
                // (empty GitStatus if even the FS read fails).
                _ => {
                    let gs = read_git_branch(Path::new(&cwd))
                        .map(|b| GitStatus { branch: Some(b), ..Default::default() })
                        .unwrap_or_default();
                    (cwd, gs)
                }
            }
        },
        |(cwd, gs)| crate::iced_app::Message::GitStatusResult(cwd, gs),
    )
}

async fn short_sha(cwd: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["-C", cwd, "rev-parse", "--short", "HEAD"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Parse `git status --porcelain=v2 --branch` output. Pure + unit-tested.
///
/// Lines of interest:
/// - `# branch.head <name>` — symbolic branch (`(detached)` if detached).
/// - `# branch.ab +<a> -<b>` — ahead/behind upstream (absent when no upstream).
/// - any `1 ...`, `2 ...`, `u ...`, `? ...` line — a changed entry ⇒ dirty.
pub fn parse_porcelain_v2(s: &str) -> GitStatus {
    let mut g = GitStatus::default();
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            g.branch = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // Format: "+2 -0"
            let mut it = rest.split_whitespace();
            if let Some(a) = it
                .next()
                .and_then(|t| t.strip_prefix('+'))
                .and_then(|n| n.parse::<usize>().ok())
            {
                g.ahead = Some(a);
            }
            if let Some(b) = it
                .next()
                .and_then(|t| t.strip_prefix('-'))
                .and_then(|n| n.parse::<usize>().ok())
            {
                g.behind = Some(b);
            }
        } else if !line.starts_with('#') && !line.is_empty() {
            // A real changed entry (changed/staged/unmerged/untracked).
            g.dirty = true;
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_head_branch() {
        assert_eq!(parse_head("ref: refs/heads/main\n"), Some("main".to_string()));
        assert_eq!(
            parse_head("ref: refs/heads/feature/status-bar\n"),
            Some("status-bar".to_string())
        );
    }

    #[test]
    fn parse_head_detached() {
        assert_eq!(
            parse_head("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2\n"),
            Some("a1b2c3d".to_string())
        );
    }

    #[test]
    fn parse_head_invalid() {
        assert_eq!(parse_head(""), None);
        assert_eq!(parse_head("not-a-ref"), None);
    }

    fn v2(branch: &str, entries: &[&str]) -> String {
        let mut s = String::new();
        s.push_str("# branch.oid 0000000000000000000000000000000000000000\n");
        s.push_str(&format!("# branch.head {}\n", branch));
        for e in entries {
            s.push_str(e);
            s.push('\n');
        }
        s
    }

    #[test]
    fn porcelain_clean_repo() {
        let out = v2("main", &[]);
        let g = parse_porcelain_v2(&out);
        assert_eq!(g.branch.as_deref(), Some("main"));
        assert!(!g.dirty);
        assert_eq!(g.ahead, None);
        assert_eq!(g.behind, None);
    }

    #[test]
    fn porcelain_dirty() {
        let out = v2("main", &["1 .M N... 100644 100644 100644 abc def file.rs"]);
        let g = parse_porcelain_v2(&out);
        assert!(g.dirty);
    }

    #[test]
    fn porcelain_untracked_is_dirty() {
        let out = v2("main", &["? new.txt"]);
        let g = parse_porcelain_v2(&out);
        assert!(g.dirty);
    }

    #[test]
    fn porcelain_ahead_behind() {
        let mut s = String::new();
        s.push_str("# branch.oid 0000000000000000000000000000000000000000\n");
        s.push_str("# branch.head main\n");
        s.push_str("# branch.ab +2 -1\n");
        let g = parse_porcelain_v2(&s);
        assert_eq!(g.ahead, Some(2));
        assert_eq!(g.behind, Some(1));
    }

    #[test]
    fn porcelain_no_upstream_has_no_ab() {
        let out = v2("main", &[]);
        let g = parse_porcelain_v2(&out);
        assert_eq!(g.ahead, None);
        assert_eq!(g.behind, None);
    }

    #[test]
    fn porcelain_detached_head() {
        let out = v2("(detached)", &[]);
        let g = parse_porcelain_v2(&out);
        assert_eq!(g.branch.as_deref(), Some("(detached)"));
    }

    #[test]
    fn porcelain_empty_is_non_git() {
        let g = parse_porcelain_v2("");
        assert_eq!(g.branch, None);
        assert!(!g.dirty);
    }
}
