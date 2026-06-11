//! Status bar utilities (GAP-3.4).
//!
//! Pure-filesystem helpers for the bottom status bar: git branch lookup,
//! foreground process name detection.

use std::path::{Path, PathBuf};

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

/// Get foreground process name for a given PID.
#[cfg(target_os = "macos")]
pub fn get_foreground_process(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(target_os = "linux")]
pub fn get_foreground_process(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/comm", pid);
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_head_branch() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n"),
            Some("main".to_string())
        );
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
}
