//! Inline command history menu.
//! Shows a popup when pressing Up arrow with matching history entries.

use std::collections::VecDeque;

/// Maximum number of history entries to keep.
const MAX_HISTORY: usize = 10000;

/// Command history with search.
#[derive(Debug, Clone)]
pub struct CommandHistory {
    /// All history entries (most recent first).
    entries: VecDeque<String>,
    /// Current search query.
    query: Option<String>,
    /// Filtered results.
    filtered: Vec<usize>, // indices into entries
    /// Current selection index in filtered.
    selected: Option<usize>,
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_HISTORY),
            query: None,
            filtered: Vec::new(),
            selected: None,
        }
    }
}

impl CommandHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a command to history. Duplicates of the most recent entry are skipped.
    pub fn push(&mut self, command: String) {
        if command.trim().is_empty() {
            return;
        }
        if self
            .entries
            .front()
            .map(|e| e == &command)
            .unwrap_or(false)
        {
            return;
        }
        self.entries.push_front(command);
        while self.entries.len() > MAX_HISTORY {
            self.entries.pop_back();
        }
    }

    /// Start a search with the given prefix.
    pub fn start_search(&mut self, prefix: &str) {
        self.query = Some(prefix.to_string());
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, cmd)| cmd.starts_with(prefix))
            .map(|(i, _)| i)
            .collect();
        self.selected = if self.filtered.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    /// Move to the next (older) entry.
    pub fn next(&mut self) -> Option<&str> {
        if self.filtered.is_empty() {
            return None;
        }
        let current = self.selected.unwrap_or(0);
        let next = if current + 1 < self.filtered.len() {
            current + 1
        } else {
            current
        };
        self.selected = Some(next);
        self.entries.get(self.filtered[next]).map(|s| s.as_str())
    }

    /// Move to the previous (newer) entry.
    pub fn prev(&mut self) -> Option<&str> {
        if self.filtered.is_empty() {
            return None;
        }
        let current = self.selected.unwrap_or(0);
        let prev = if current > 0 { current - 1 } else { 0 };
        self.selected = Some(prev);
        self.entries.get(self.filtered[prev]).map(|s| s.as_str())
    }

    /// Cancel the search and return the original prefix.
    pub fn cancel(&mut self) -> Option<String> {
        self.query.take()
    }

    /// Get the currently selected entry.
    pub fn selected_entry(&self) -> Option<&str> {
        let idx = self.selected?;
        let entry_idx = self.filtered.get(idx)?;
        self.entries.get(*entry_idx).map(|s| s.as_str())
    }

    /// Check if search is active.
    pub fn is_searching(&self) -> bool {
        self.query.is_some()
    }

    /// Get the number of filtered results.
    pub fn filtered_count(&self) -> usize {
        self.filtered.len()
    }

    /// Get all entries (for persistence).
    pub fn entries(&self) -> &VecDeque<String> {
        &self.entries
    }

    /// Load entries from storage.
    pub fn load_entries(&mut self, entries: Vec<String>) {
        self.entries = entries.into_iter().collect();
        while self.entries.len() > MAX_HISTORY {
            self.entries.pop_back();
        }
    }

    /// Save history to a file.
    pub fn save_to_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        let content: String = self
            .entries
            .iter()
            .map(|e| e.replace('\n', "\\n"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, content)
    }

    /// Load history from a file.
    pub fn load_from_file(path: &std::path::Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let entries: Vec<String> = content.lines().map(|l| l.replace("\\n", "\n")).collect();
        let mut history = Self::new();
        // Reverse so oldest is first, push reverses to most-recent-first
        for entry in entries.into_iter().rev() {
            history.push(entry);
        }
        Ok(history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_entries() {
        let mut h = CommandHistory::new();
        h.push("ls".into());
        h.push("cd /tmp".into());
        h.push("git status".into());
        assert_eq!(h.entries().len(), 3);
        assert_eq!(h.entries()[0], "git status");
        assert_eq!(h.entries()[2], "ls");
    }

    #[test]
    fn test_push_skips_empty() {
        let mut h = CommandHistory::new();
        h.push("".into());
        h.push("  ".into());
        assert_eq!(h.entries().len(), 0);
    }

    #[test]
    fn test_push_skips_consecutive_duplicate() {
        let mut h = CommandHistory::new();
        h.push("ls".into());
        h.push("ls".into());
        assert_eq!(h.entries().len(), 1);
    }

    #[test]
    fn test_push_allows_non_consecutive_duplicate() {
        let mut h = CommandHistory::new();
        h.push("ls".into());
        h.push("cd".into());
        h.push("ls".into());
        assert_eq!(h.entries().len(), 3);
        assert_eq!(h.entries()[0], "ls"); // most recent
    }

    #[test]
    fn test_start_search_and_navigation() {
        let mut h = CommandHistory::new();
        h.push("ls -la".into());
        h.push("git status".into());
        h.push("ls /tmp".into());
        h.push("git log".into());

        h.start_search("git");
        assert_eq!(h.filtered_count(), 2);
        assert_eq!(h.selected_entry(), Some("git log")); // most recent git cmd

        let next = h.next();
        assert_eq!(next, Some("git status"));

        let prev = h.prev();
        assert_eq!(prev, Some("git log"));
    }

    #[test]
    fn test_cancel_search() {
        let mut h = CommandHistory::new();
        h.push("ls".into());
        h.start_search("l");
        assert!(h.is_searching());
        let prefix = h.cancel();
        assert_eq!(prefix, Some("l".to_string()));
        assert!(!h.is_searching());
    }

    #[test]
    fn test_max_history_limit() {
        let mut h = CommandHistory::new();
        for i in 0..(MAX_HISTORY + 100) {
            h.push(format!("cmd_{}", i));
        }
        assert_eq!(h.entries().len(), MAX_HISTORY);
        // Most recent should be the last pushed
        assert_eq!(h.entries()[0], format!("cmd_{}", MAX_HISTORY + 99));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.txt");

        let mut h = CommandHistory::new();
        h.push("ls -la".into());
        h.push("git status".into());
        h.save_to_file(&path).unwrap();

        let loaded = CommandHistory::load_from_file(&path).unwrap();
        assert_eq!(loaded.entries().len(), 2);
        assert_eq!(loaded.entries()[0], "git status");
        assert_eq!(loaded.entries()[1], "ls -la");
    }

    #[test]
    fn test_save_and_load_with_newlines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.txt");

        let mut h = CommandHistory::new();
        h.push("echo 'line1\nline2'".into());
        h.save_to_file(&path).unwrap();

        let loaded = CommandHistory::load_from_file(&path).unwrap();
        assert_eq!(loaded.entries().len(), 1);
        assert_eq!(loaded.entries()[0], "echo 'line1\nline2'");
    }

    #[test]
    fn test_load_entries() {
        let mut h = CommandHistory::new();
        h.load_entries(vec!["old1".into(), "old2".into(), "old3".into()]);
        assert_eq!(h.entries().len(), 3);
        // Vec -> VecDeque preserves order: first element is "old1"
        assert_eq!(h.entries()[0], "old1");
    }
}
