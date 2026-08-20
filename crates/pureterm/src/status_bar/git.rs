//! GAP-24：状态栏的 Git 状态。
//!
//! 两层：
//! - `read_git_branch` —— 廉价的同步纯文件系统分支查找（无 `git`
//!   子进程）。从旧的 `status_bar.rs` 原样保留，使即时 OSC7 / 面板切换
//!   路径永不回归，并在未安装 `git` 时提供回退。
//! - `spawn_git_status` —— 异步 porcelain-v2 升级：分支 + 脏标记 +
//!   领先/落后 一次子进程完成，通过 `Task::perform` 在 GUI 线程之外运行。
//!
//! `parse_porcelain_v2` 是纯函数，已单元测试。

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

/// 进行中的 git 操作，从 `.git/` 文件系统标记检测
/// （无子进程）。优先级顺序由 [`detect_git_op`] 应用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GitOp {
    /// 没有进行中的操作。
    #[default]
    None,
    Rebase,
    Merge,
    CherryPick,
    Bisect,
    Revert,
}

impl GitOp {
    /// 供状态栏图形 / 面板提示使用的短小写标签。
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

/// 供状态栏（以及 GAP-32 Git 面板）使用的增强型 Git 状态。
///
/// 原来的 GAP-24 字段（`branch`/`dirty`/`ahead`/`behind`）语义保持不变；
/// `dirty` 现在由各类别计数推导。GAP-32 增加了 staged/unstaged/untracked/
/// unmerged/renamed 计数、上游、HEAD oid、进行中的操作，以及关联的 PR
/// （由 [`crate::gh_pr`] 填充，不是 porcelain）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub dirty: bool,
    /// 领先上游的提交数（没有上游时为 `None`）。
    pub ahead: Option<usize>,
    /// 落后上游的提交数。
    pub behind: Option<usize>,
    /// 已暂存（索引）的变更——porcelain `1 X.`、`2 …`（重命名/复制）。
    pub staged: usize,
    /// 未暂存（工作树）的变更——porcelain `1 .Y`。
    pub unstaged: usize,
    /// 未跟踪文件——porcelain `? …`。
    pub untracked: usize,
    /// 未合并 / 冲突条目——porcelain `u …`。
    pub unmerged: usize,
    /// 重命名/复制条目（`staged` 的子集）。
    pub renamed: usize,
    /// 上游跟踪分支（`# branch.upstream`），例如 `origin/main`。
    pub upstream: Option<String>,
    /// 短的 HEAD oid（`# branch.oid`，前 7 个字符）。
    pub head_short: Option<String>,
    /// 来自 `.git/` 标记的进行中操作。
    pub op: GitOp,
    /// 来自 `gh` 的关联 PR（唯一事实来源：状态栏胶囊 + 面板）。
    pub pr: Option<crate::gh_pr::PrInfo>,
}

/// 廉价的同步 git 分支查找。从 `start` 向上查找 `.git/HEAD`；若找到，
/// 解析 `ref: refs/heads/<名称>` 并返回分支名。若 HEAD 直接指向 40 字符
/// SHA（分离头），返回前 7 个字符。
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
        // 同时处理 `start` 本身就是 `.git` 目录的情况。
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
        // refs/heads/<名称>
        let name = r.rsplit('/').next().unwrap_or(r);
        if name.is_empty() {
            return None;
        }
        return Some(name.to_string());
    }
    // 分离头：期望 40 字符的十六进制 SHA。
    if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(trimmed.chars().take(7).collect());
    }
    None
}

/// 从 `start` 向上查找 `.git` 目录（GAP-32）。处理普通的 `.git/` 目录、
/// `.git` *文件*（关联工作树——内容 `gitdir: <路径>`），以及 `start`
/// 本身就是 `.git` 目录的情况。纯文件系统读取；在任何仓库之外返回
/// `None`。
pub fn find_git_dir(start: &Path) -> Option<PathBuf> {
    let mut current: Option<&Path> = Some(start);
    while let Some(dir) = current {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            // 关联工作树：`.git` 文件指向真正的 git 目录。
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
        // `start` 本身可能是 `.git` 目录（例如类裸仓库调用）。
        if dir.file_name().and_then(|n| n.to_str()) == Some(".git") && dir.is_dir() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// 从 `.git/` 文件系统标记检测进行中的 git 操作（GAP-32）。
/// 纯文件系统读取（无子进程）。优先级：先匹配者胜出。
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

/// 零依赖探测：某二进制在 `PATH` 上可调用吗？（GAP-32；用于 `gh` /
/// `lazygit`）。与项目的内联 `Command` 约定一致——不用 `which` crate。
/// 若进程*能生成*（存在）则返回 `true`，不关心其退出码，因为少数 CLI
/// 在 `--version` 时非零退出。
pub fn is_binary_available(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// 启动异步的 git 状态升级。在 GUI 线程之外运行
/// `git status --porcelain=v2 --branch`；把结果映射回
/// [`Message::GitStatusResult`]。若 `git` 缺失或目录不是仓库，回退到
/// 纯文件系统分支读取，使行为永不低于当前的状态栏。
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
                    // GAP-32：用进行中的操作增强（纯文件系统）。
                    if let Some(git_dir) = find_git_dir(Path::new(&cwd)) {
                        gs.op = detect_git_op(&git_dir);
                    }
                    // 分离 HEAD：porcelain 输出 “# branch.head (detached)”；
                    // 解析出短 SHA，使状态栏显示有用的内容。
                    if gs.branch.as_deref() == Some("(detached)") {
                        gs.branch = short_sha(&cwd)
                            .await
                            .or_else(|| read_git_branch(Path::new(&cwd)));
                    }
                    (cwd, gs)
                }
                // git 未安装 / 不是仓库 / 非零退出 → 纯文件系统回退
                // （连文件系统读取也失败时返回空的 GitStatus）。
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

/// 解析 `git status --porcelain=v2 --branch` 输出。纯函数 + 已单元测试。
///
/// 关注的行：
/// - `# branch.head <名称>` —— 符号分支（分离时为 `(detached)`）。
/// - `# branch.upstream <名称>` —— 上游跟踪分支（GAP-32）。
/// - `# branch.oid <sha>` —— HEAD oid → `head_short`（前 7 字符）（GAP-32）。
/// - `# branch.ab +<a> -<b>` —— 领先/落后上游（无上游时缺省）。
/// - `1 XY …` —— 已跟踪变更。2 字符 `XY` 字段区分索引与工作树：
///   `X` ⇒ 已暂存（重命名/复制的 `R`/`C` 也会增加 `renamed`），`Y` ⇒ 未暂存。
///   既暂存又修改的文件（`1 MM`）两个计数器都增加。
/// - `2 …` —— 索引中的重命名/复制 ⇒ staged + renamed。
/// - `u …` —— 未合并 / 冲突。
/// - `? …` —— 未跟踪。
///
/// `dirty` 在循环后推导为 `staged + unstaged + untracked + unmerged > 0`
/// （对现有状态栏消费者行为不变）。
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
            // 格式：“+2 -0”
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
            // `1 XY sub mH mI mW hH hI path` — XY 是 2 字符状态字段。
            let mut parts = line.split_whitespace();
            parts.next(); // 消费 "1"
            if let Some(xy) = parts.next() {
                let bytes = xy.as_bytes();
                if bytes.len() >= 2 {
                    let (x, y) = (bytes[0] as char, bytes[1] as char);
                    // 索引（已暂存）。`.`/` ` 意为“索引无变化”。
                    if x != ' ' && x != '.' {
                        g.staged += 1;
                        if x == 'R' || x == 'C' {
                            g.renamed += 1;
                        }
                    }
                    // 工作树（未暂存）。
                    if y != ' ' && y != '.' {
                        g.unstaged += 1;
                    }
                }
            }
        } else if line.starts_with("2 ") {
            // 索引中的重命名/复制。
            g.staged += 1;
            g.renamed += 1;
        } else if line.starts_with("u ") {
            g.unmerged += 1;
        } else if let Some(_path) = line.strip_prefix("? ") {
            g.untracked += 1;
        } else if line.starts_with('?') {
            // 无空格的裸 `?`（v2 中不应出现，但保持宽容）。
            g.untracked += 1;
        } else if !line.starts_with('#') && !line.is_empty() {
            // 未知的条目类型——保守地计为工作树变更。
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

    // ── GAP-32：各类别计数 ────────────────────────────────────────

    #[test]
    fn porcelain_staged_unstaged_split() {
        // `1 M.` → 仅已暂存。
        let g = parse_porcelain_v2(&v2("main", &["1 M. N... 100644 100644 100644 abc def a.rs"]));
        assert_eq!((g.staged, g.unstaged), (1, 0));
        // `1 .M` → 仅未暂存。
        let g = parse_porcelain_v2(&v2("main", &["1 .M N... 100644 100644 100644 abc def b.rs"]));
        assert_eq!((g.staged, g.unstaged), (0, 1));
        // `1 MM` → 既暂存又未暂存（一个文件，两个计数器）。
        let g = parse_porcelain_v2(&v2("main", &["1 MM N... 100644 100644 100644 abc def c.rs"]));
        assert_eq!((g.staged, g.unstaged), (1, 1));
    }

    #[test]
    fn porcelain_renamed() {
        // `2 …` 索引中的重命名 → staged + renamed。
        let g = parse_porcelain_v2(&v2("main", &["2 R. N... 100644 100644 100644 abc def old\tnew"]));
        assert_eq!((g.staged, g.renamed), (1, 1));
        // `1 R.` 已跟踪的暂存重命名。
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

    // ── GAP-32：操作检测 + git 目录解析（FS、tempdir）────────────

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
        // 关联工作树：`.git` *文件*指向真正的 git 目录。
        let wt = dir.path().join("worktree");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", real_gd.display())).unwrap();
        assert_eq!(find_git_dir(&wt), Some(real_gd));
    }
}
