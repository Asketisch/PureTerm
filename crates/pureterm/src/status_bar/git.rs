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

/// An in-progress git operation, detected from `.git/` filesystem markers
/// (no subprocess). Priority order is applied by [`detect_git_op`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GitOp {
    /// No operation in progress.
    #[default]
    None,
    Rebase,
    Merge,
    CherryPick,
    Bisect,
    Revert,
}

impl GitOp {
    /// Short lower-case label for the status-bar glyph / panel hint.
    pub fn as_str(self) -> &'static str {
        match self {
            GitOp::None => "",
            GitOp::Rebase => "rebase",
            GitOp::Merge => "merge",
            GitOp::CherryPick => "cherry-pick",
            GitOp::Bisect => "bisect",
            GitOp::Revert => "revert",
        }
    }
}

/// Enriched Git status for the status bar (and the GAP-32 Git panel).
///
/// The original GAP-24 fields (`branch`/`dirty`/`ahead`/`behind`) keep their
/// semantics; `dirty` is now derived from the per-category counts. GAP-32 adds
/// staged/unstaged/untracked/unmerged/renamed counts, upstream, HEAD oid, the
/// in-progress op, and a linked PR (filled by [`crate::gh_pr`], not porcelain).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub dirty: bool,
    /// Commits ahead of upstream (`None` when there is no upstream).
    pub ahead: Option<usize>,
    /// Commits behind upstream.
    pub behind: Option<usize>,
    /// Staged (index) changes — porcelain `1 X.`, `2 …` (renames/copies).
    pub staged: usize,
    /// Unstaged (worktree) changes — porcelain `1 .Y`.
    pub unstaged: usize,
    /// Untracked files — porcelain `? …`.
    pub untracked: usize,
    /// Unmerged / conflicted entries — porcelain `u …`.
    pub unmerged: usize,
    /// Renamed/copied entries (a subset of `staged`).
    pub renamed: usize,
    /// Upstream tracking branch (`# branch.upstream`), e.g. `origin/main`.
    pub upstream: Option<String>,
    /// Short HEAD oid (`# branch.oid`, first 7 chars).
    pub head_short: Option<String>,
    /// In-progress operation from `.git/` markers.
    pub op: GitOp,
    /// Linked PR from `gh` (single source of truth: status-bar pill + panel).
    pub pr: Option<crate::gh_pr::PrInfo>,
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

/// Walk up from `start` to locate the `.git` directory (GAP-32). Handles a
/// normal `.git/` dir, a `.git` *file* (linked worktrees — contents
/// `gitdir: <path>`), and the case where `start` is itself a `.git` dir. Pure
/// filesystem reads; returns `None` outside any repo.
pub fn find_git_dir(start: &Path) -> Option<PathBuf> {
    let mut current: Option<&Path> = Some(start);
    while let Some(dir) = current {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            // Linked worktree: `.git` file points at the real git dir.
            if let Ok(raw) = std::fs::read_to_string(&candidate) {
                if let Some(p) = raw.trim().strip_prefix("gitdir:") {
                    let p = p.trim();
                    let resolved = if Path::new(p).is_absolute() {
                        PathBuf::from(p)
                    } else {
                        candidate
                            .parent()
                            .unwrap_or(Path::new("."))
                            .join(p)
                    };
                    if resolved.is_dir() {
                        return Some(resolved);
                    }
                }
            }
        }
        // `start` itself might be a `.git` dir (e.g. bare-ish invocations).
        if dir.file_name().and_then(|n| n.to_str()) == Some(".git") && dir.is_dir() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// Detect an in-progress git operation from `.git/` filesystem markers (GAP-32).
/// Pure filesystem reads (no subprocess). Priority: first match wins.
pub fn detect_git_op(git_dir: &Path) -> GitOp {
    if git_dir.join("rebase-merge").is_dir() || git_dir.join("rebase-apply").is_dir() {
        return GitOp::Rebase;
    }
    if git_dir.join("MERGE_HEAD").is_file() {
        return GitOp::Merge;
    }
    if git_dir.join("CHERRY_PICK_HEAD").is_file() {
        return GitOp::CherryPick;
    }
    if git_dir.join("BISECT_LOG").is_file() {
        return GitOp::Bisect;
    }
    if git_dir.join("REVERT_HEAD").is_file() {
        return GitOp::Revert;
    }
    GitOp::None
}

/// Zero-dependency probe: is a binary invokable on `PATH`? (GAP-32; used for
/// `gh` / `lazygit`). Matches the project's inline-`Command` convention — no
/// `which` crate. Returns `true` if the process *spawns* (exists), regardless of
/// its exit code, since a few CLIs exit non-zero on `--version`.
pub fn is_binary_available(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
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
                    // GAP-32: enrich with the in-progress operation (pure FS).
                    if let Some(git_dir) = find_git_dir(Path::new(&cwd)) {
                        gs.op = detect_git_op(&git_dir);
                    }
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
/// - `# branch.upstream <name>` — upstream tracking branch (GAP-32).
/// - `# branch.oid <sha>` — HEAD oid → `head_short` (first 7 chars) (GAP-32).
/// - `# branch.ab +<a> -<b>` — ahead/behind upstream (absent when no upstream).
/// - `1 XY …` — tracked change. The 2-char `XY` field splits index vs worktree:
///   `X` ⇒ staged (a renamed/copied `R`/`C` also bumps `renamed`), `Y` ⇒ unstaged.
///   A file both staged and modified (`1 MM`) bumps both counters.
/// - `2 …` — renamed/copied in the index ⇒ staged + renamed.
/// - `u …` — unmerged / conflicted.
/// - `? …` — untracked.
///
/// `dirty` is derived post-loop as `staged + unstaged + untracked + unmerged > 0`
/// (unchanged behaviour for existing status-bar consumers).
pub fn parse_porcelain_v2(s: &str) -> GitStatus {
    let mut g = GitStatus::default();
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            g.branch = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            let u = rest.trim();
            if !u.is_empty() {
                g.upstream = Some(u.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.oid ") {
            let oid = rest.trim();
            if oid.len() >= 7 && oid.chars().all(|c| c.is_ascii_hexdigit()) {
                g.head_short = Some(oid.chars().take(7).collect());
            }
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
        } else if line.starts_with("1 ") {
            // `1 XY sub mH mI mW hH hI path` — XY is the 2-char status field.
            let mut parts = line.split_whitespace();
            parts.next(); // consume "1"
            if let Some(xy) = parts.next() {
                let bytes = xy.as_bytes();
                if bytes.len() >= 2 {
                    let (x, y) = (bytes[0] as char, bytes[1] as char);
                    // Index (staged). `.`/` ` mean "no change in index".
                    if x != ' ' && x != '.' {
                        g.staged += 1;
                        if x == 'R' || x == 'C' {
                            g.renamed += 1;
                        }
                    }
                    // Worktree (unstaged).
                    if y != ' ' && y != '.' {
                        g.unstaged += 1;
                    }
                }
            }
        } else if line.starts_with("2 ") {
            // Renamed/copied in the index.
            g.staged += 1;
            g.renamed += 1;
        } else if line.starts_with("u ") {
            g.unmerged += 1;
        } else if let Some(_path) = line.strip_prefix("? ") {
            g.untracked += 1;
        } else if line.starts_with('?') {
            // Bare `?` with no space (shouldn't happen in v2, but be lenient).
            g.untracked += 1;
        } else if !line.starts_with('#') && !line.is_empty() {
            // Unknown entry type — conservatively count as a worktree change.
            g.unstaged += 1;
        }
    }
    g.dirty = g.staged + g.unstaged + g.untracked + g.unmerged > 0;
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

    // ── GAP-32: per-category counts ────────────────────────────────────────

    #[test]
    fn porcelain_staged_unstaged_split() {
        // `1 M.` → staged only.
        let g = parse_porcelain_v2(&v2("main", &["1 M. N... 100644 100644 100644 abc def a.rs"]));
        assert_eq!((g.staged, g.unstaged), (1, 0));
        // `1 .M` → unstaged only.
        let g = parse_porcelain_v2(&v2("main", &["1 .M N... 100644 100644 100644 abc def b.rs"]));
        assert_eq!((g.staged, g.unstaged), (0, 1));
        // `1 MM` → both staged and unstaged (one file, two counters).
        let g = parse_porcelain_v2(&v2("main", &["1 MM N... 100644 100644 100644 abc def c.rs"]));
        assert_eq!((g.staged, g.unstaged), (1, 1));
    }

    #[test]
    fn porcelain_renamed() {
        // `2 …` rename in index → staged + renamed.
        let g = parse_porcelain_v2(&v2("main", &["2 R. N... 100644 100644 100644 abc def old\tnew"]));
        assert_eq!((g.staged, g.renamed), (1, 1));
        // `1 R.` tracked staged rename.
        let g = parse_porcelain_v2(&v2("main", &["1 R. N... 100644 100644 100644 abc def x.rs"]));
        assert_eq!((g.staged, g.renamed), (1, 1));
    }

    #[test]
    fn porcelain_unmerged() {
        let g = parse_porcelain_v2(&v2("main", &["u AA N... 100644 100644 100644 abc def conflicted.rs"]));
        assert_eq!(g.unmerged, 1);
        assert!(g.dirty);
    }

    #[test]
    fn porcelain_untracked_count() {
        let g = parse_porcelain_v2(&v2("main", &["? a.txt", "? b.txt", "? c.txt"]));
        assert_eq!(g.untracked, 3);
        assert_eq!((g.staged, g.unstaged), (0, 0));
        assert!(g.dirty);
    }

    #[test]
    fn porcelain_upstream_parsed() {
        let s = "# branch.oid abcdef0123456789abcdef0123456789abcdef01\n\
                 # branch.head main\n\
                 # branch.upstream origin/main\n\
                 # branch.ab +0 -0\n";
        let g = parse_porcelain_v2(s);
        assert_eq!(g.upstream.as_deref(), Some("origin/main"));
    }

    #[test]
    fn porcelain_head_oid() {
        let s = "# branch.oid abcdef0123456789abcdef0123456789abcdef01\n\
                 # branch.head main\n";
        let g = parse_porcelain_v2(s);
        assert_eq!(g.head_short.as_deref(), Some("abcdef0"));
    }

    // ── GAP-32: op detection + git-dir resolution (FS, tempdir) ────────────

    #[test]
    fn op_detect_markers() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let gd = dir.path().join(".git");
        fs::create_dir_all(&gd).unwrap();

        assert_eq!(detect_git_op(&gd), GitOp::None);

        fs::write(gd.join("MERGE_HEAD"), "abc\n").unwrap();
        assert_eq!(detect_git_op(&gd), GitOp::Merge);
        fs::remove_file(gd.join("MERGE_HEAD")).unwrap();

        fs::write(gd.join("CHERRY_PICK_HEAD"), "abc\n").unwrap();
        assert_eq!(detect_git_op(&gd), GitOp::CherryPick);
        fs::remove_file(gd.join("CHERRY_PICK_HEAD")).unwrap();

        fs::write(gd.join("BISECT_LOG"), "x\n").unwrap();
        assert_eq!(detect_git_op(&gd), GitOp::Bisect);
        fs::remove_file(gd.join("BISECT_LOG")).unwrap();

        fs::write(gd.join("REVERT_HEAD"), "abc\n").unwrap();
        assert_eq!(detect_git_op(&gd), GitOp::Revert);
        fs::remove_file(gd.join("REVERT_HEAD")).unwrap();

        fs::create_dir_all(gd.join("rebase-merge")).unwrap();
        assert_eq!(detect_git_op(&gd), GitOp::Rebase);
    }

    #[test]
    fn find_git_dir_walks_up() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let gd = dir.path().join(".git");
        fs::create_dir_all(&gd).unwrap();
        let nested = dir.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_git_dir(&nested), Some(gd));
    }

    #[test]
    fn find_git_dir_worktree_file() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let real_gd = dir.path().join("real.git");
        fs::create_dir_all(&real_gd).unwrap();
        // Linked worktree: a `.git` *file* pointing at the real git dir.
        let wt = dir.path().join("worktree");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", real_gd.display())).unwrap();
        assert_eq!(find_git_dir(&wt), Some(real_gd));
    }
}
