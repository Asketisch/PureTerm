//! SFTP file browser navigation + selection state.
//!
//! Pure synchronous data layer modeled on iTerm2's SSHFilePanel. The pane/UI
//! layer drives async [`pureterm_session::ssh::sftp::SftpClient`] calls and
//! feeds the results back via [`SftpBrowserState::set_listing`].
//!
//! This module deliberately performs no I/O — all sorting, filtering and
//! navigation is in-memory so it can be unit-tested without a live SSH
//! connection.

use std::collections::HashSet;

use pureterm_session::ssh::sftp::RemoteFileMeta;

/// Sort mode for the visible directory listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    Name,
    NameDesc,
    Size,
    SizeDesc,
    Modified,
    ModifiedDesc,
}

impl Default for SortMode {
    fn default() -> Self {
        SortMode::Name
    }
}

/// Browser state: cwd, entries, history stacks, selection, view options.
#[derive(Default, Debug, Clone)]
pub struct SftpBrowserState {
    /// The directory whose contents are currently shown.
    pub cwd: String,
    /// Entries (after sort + filter applied) currently visible.
    pub entries: Vec<RemoteFileMeta>,
    /// User's directory back-stack (most recent at end).
    pub history: Vec<String>,
    /// Forward stack used after Back.
    pub forward: Vec<String>,
    /// Selected entries (by absolute path).
    pub selection: HashSet<String>,
    /// Substring filter (case-insensitive).
    pub filter: String,
    /// Sort mode.
    pub sort: SortMode,
    /// Show hidden files (those starting with '.').
    pub show_hidden: bool,
    /// In-flight async op marker (for UI spinner).
    pub loading: bool,
    /// Last error string (None on success).
    pub last_error: Option<String>,
}

impl SftpBrowserState {
    /// Create a new browser state rooted at `initial_cwd`.
    pub fn new(initial_cwd: String) -> Self {
        Self {
            cwd: initial_cwd,
            ..Self::default()
        }
    }

    /// Replace the visible entries (called after a directory listing comes
    /// back). Applies sort + filter + show_hidden.
    pub fn set_listing(&mut self, raw_entries: Vec<RemoteFileMeta>) {
        let mut filtered: Vec<RemoteFileMeta> = raw_entries
            .into_iter()
            .filter(|e| e.name != "." && e.name != "..")
            .filter(|e| self.show_hidden || !e.name.starts_with('.'))
            .collect();

        if !self.filter.is_empty() {
            let needle = self.filter.to_lowercase();
            filtered.retain(|e| e.name.to_lowercase().contains(&needle));
        }

        sort_entries(&mut filtered, self.sort);
        self.entries = filtered;
    }

    /// Navigate into a sub-path (relative or absolute).
    /// Pushes the previous cwd onto history and clears forward.
    pub fn navigate(&mut self, target: &str) {
        if target == ".." {
            self.navigate_up();
            return;
        }
        let new_cwd = resolve_path(&self.cwd, target);
        if new_cwd == self.cwd {
            return;
        }
        let prev = std::mem::replace(&mut self.cwd, new_cwd);
        self.history.push(prev);
        self.forward.clear();
        self.selection.clear();
    }

    /// Go up one level (cwd → parent). No-op at `/`.
    pub fn navigate_up(&mut self) {
        let parent = parent_of(&self.cwd);
        if parent == self.cwd {
            return;
        }
        let prev = std::mem::replace(&mut self.cwd, parent);
        self.history.push(prev);
        self.forward.clear();
        self.selection.clear();
    }

    /// Pop the back-stack. Pushes the current cwd onto the forward stack.
    pub fn back(&mut self) {
        if let Some(prev) = self.history.pop() {
            let cur = std::mem::replace(&mut self.cwd, prev);
            self.forward.push(cur);
            self.selection.clear();
        }
    }

    /// Pop the forward-stack. Pushes the current cwd onto the back-stack.
    pub fn forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            let cur = std::mem::replace(&mut self.cwd, next);
            self.history.push(cur);
            self.selection.clear();
        }
    }

    /// Toggle selection of the given path.
    pub fn toggle_select(&mut self, path: &str) {
        if !self.selection.remove(path) {
            self.selection.insert(path.to_string());
        }
    }

    /// Select every currently-visible entry.
    pub fn select_all(&mut self) {
        self.selection = self.entries.iter().map(|e| e.path.clone()).collect();
    }

    /// Drop all selections.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Update the substring filter. Caller should re-issue `set_listing` with
    /// the cached raw listing to apply the filter.
    pub fn set_filter(&mut self, q: String) {
        self.filter = q;
    }

    /// Update the sort mode. Caller should re-issue `set_listing` (or call
    /// `resort` if it caches the raw listing) to apply.
    pub fn set_sort(&mut self, mode: SortMode) {
        self.sort = mode;
    }

    /// Toggle visibility of dotfiles. Caller should re-issue `set_listing`.
    pub fn set_show_hidden(&mut self, on: bool) {
        self.show_hidden = on;
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn sort_entries(entries: &mut [RemoteFileMeta], mode: SortMode) {
    match mode {
        SortMode::Name => {
            entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }
        SortMode::NameDesc => {
            entries.sort_by(|a, b| b.name.to_lowercase().cmp(&a.name.to_lowercase()));
        }
        SortMode::Size => entries.sort_by(|a, b| a.size.cmp(&b.size)),
        SortMode::SizeDesc => entries.sort_by(|a, b| b.size.cmp(&a.size)),
        SortMode::Modified => entries.sort_by(|a, b| {
            // None should sort last in ascending order.
            match (a.modified, b.modified) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        }),
        SortMode::ModifiedDesc => entries.sort_by(|a, b| {
            // None still sorts last in descending order.
            match (a.modified, b.modified) {
                (Some(x), Some(y)) => y.cmp(&x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        }),
    }
}

/// Resolve a possibly-relative target against the given cwd. Absolute paths
/// (starting with `/`) replace cwd; everything else is appended.
fn resolve_path(cwd: &str, target: &str) -> String {
    if target.is_empty() {
        return cwd.to_string();
    }
    if target.starts_with('/') {
        return normalize(target);
    }
    let joined = if cwd.ends_with('/') {
        format!("{}{}", cwd, target)
    } else {
        format!("{}/{}", cwd, target)
    };
    normalize(&joined)
}

/// Return the parent directory of `path`. `/` is its own parent.
fn parent_of(path: &str) -> String {
    if path == "/" || path.is_empty() {
        return "/".to_string();
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".to_string(),
        Some(idx) => trimmed[..idx].to_string(),
        None => "/".to_string(),
    }
}

/// Collapse `.` / `..` segments and duplicate slashes in an absolute path.
fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut stack: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    if stack.is_empty() {
        return if absolute { "/".to_string() } else { ".".to_string() };
    }
    if absolute {
        format!("/{}", stack.join("/"))
    } else {
        stack.join("/")
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool, size: u64, modified: Option<u64>) -> RemoteFileMeta {
        RemoteFileMeta {
            name: name.into(),
            path: format!("/test/{}", name),
            is_dir,
            size,
            modified,
            permissions: None,
        }
    }

    #[test]
    fn set_listing_applies_name_sort() {
        let mut s = SftpBrowserState::new("/test".into());
        s.show_hidden = true;
        s.set_listing(vec![
            entry("Charlie", false, 1, None),
            entry("alpha", false, 2, None),
            entry("bravo", true, 3, None),
        ]);
        let names: Vec<&str> = s.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "bravo", "Charlie"]);
    }

    #[test]
    fn set_listing_filters_by_substring() {
        let mut s = SftpBrowserState::new("/test".into());
        s.show_hidden = true;
        s.set_filter("Log".into());
        s.set_listing(vec![
            entry("server.log", false, 1, None),
            entry("README", false, 2, None),
            entry("logs", true, 3, None),
        ]);
        let names: Vec<&str> = s.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["logs", "server.log"]);
    }

    #[test]
    fn set_listing_hides_hidden_when_disabled() {
        let mut s = SftpBrowserState::new("/test".into());
        // show_hidden defaults to false.
        s.set_listing(vec![
            entry(".bashrc", false, 1, None),
            entry("readme", false, 2, None),
            entry(".git", true, 3, None),
        ]);
        let names: Vec<&str> = s.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["readme"]);
    }

    #[test]
    fn navigate_pushes_history_and_clears_forward() {
        let mut s = SftpBrowserState::new("/home/user".into());
        s.forward.push("/tmp".into()); // pretend a stale forward entry exists.
        s.navigate("projects");
        assert_eq!(s.cwd, "/home/user/projects");
        assert_eq!(s.history, vec!["/home/user".to_string()]);
        assert!(s.forward.is_empty());
    }

    #[test]
    fn navigate_dotdot_acts_as_up() {
        let mut s = SftpBrowserState::new("/home/user/projects".into());
        s.navigate("..");
        assert_eq!(s.cwd, "/home/user");
        assert_eq!(s.history, vec!["/home/user/projects".to_string()]);
    }

    #[test]
    fn navigate_up_at_root_is_noop() {
        let mut s = SftpBrowserState::new("/".into());
        s.navigate_up();
        assert_eq!(s.cwd, "/");
        assert!(s.history.is_empty());
    }

    #[test]
    fn back_then_forward_restores_cwd() {
        let mut s = SftpBrowserState::new("/a".into());
        s.navigate("b");
        s.navigate("c");
        assert_eq!(s.cwd, "/a/b/c");

        s.back();
        assert_eq!(s.cwd, "/a/b");
        s.back();
        assert_eq!(s.cwd, "/a");

        s.forward();
        assert_eq!(s.cwd, "/a/b");
        s.forward();
        assert_eq!(s.cwd, "/a/b/c");
    }

    #[test]
    fn toggle_select_then_clear() {
        let mut s = SftpBrowserState::new("/test".into());
        s.toggle_select("/test/a");
        s.toggle_select("/test/b");
        assert!(s.selection.contains("/test/a"));
        assert!(s.selection.contains("/test/b"));

        // Toggling an existing entry removes it.
        s.toggle_select("/test/a");
        assert!(!s.selection.contains("/test/a"));
        assert!(s.selection.contains("/test/b"));

        s.clear_selection();
        assert!(s.selection.is_empty());
    }

    #[test]
    fn sort_by_size_descending() {
        let mut s = SftpBrowserState::new("/test".into());
        s.show_hidden = true;
        s.set_sort(SortMode::SizeDesc);
        s.set_listing(vec![
            entry("small", false, 10, None),
            entry("big", false, 1_000_000, None),
            entry("mid", false, 5_000, None),
        ]);
        let names: Vec<&str> = s.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["big", "mid", "small"]);
    }

    #[test]
    fn select_all_picks_visible_entries() {
        let mut s = SftpBrowserState::new("/test".into());
        s.show_hidden = true;
        s.set_listing(vec![entry("a", false, 1, None), entry("b", false, 2, None)]);
        s.select_all();
        assert_eq!(s.selection.len(), 2);
        assert!(s.selection.contains("/test/a"));
        assert!(s.selection.contains("/test/b"));
    }
}
