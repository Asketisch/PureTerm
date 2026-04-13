use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Entry tracking a directory's visit statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub path: String,
    pub visit_count: u64,
    pub last_visited: u64, // unix timestamp (seconds)
    pub alias: Option<String>,
}

/// Directory frequency tracker.
///
/// Records how often and how recently the user visits directories (typically
/// reported via OSC 7 CWD sequences) and persists the data to a JSON file.
pub struct DirFrequencyService {
    entries: HashMap<String, DirEntry>,
    max_entries: usize,
    persist_path: PathBuf,
    dirty: bool,
    save_counter: u32,
}

/// Number of visits between automatic saves.
const SAVE_INTERVAL: u32 = 10;

impl DirFrequencyService {
    /// Create a new service, loading persisted data from disk if available.
    ///
    /// Defaults to a maximum of 500 entries and a persist path of
    /// `~/.config/pureterm/dirfreq.json`.
    pub fn new() -> Self {
        let persist_path = Self::default_persist_path();
        Self::with_persist_path(persist_path)
    }

    /// Create a service with a custom persist path (useful for testing).
    pub fn with_persist_path(persist_path: PathBuf) -> Self {
        let entries = Self::load(&persist_path).unwrap_or_default();
        Self {
            entries,
            max_entries: 500,
            persist_path,
            dirty: false,
            save_counter: 0,
        }
    }

    /// Record a visit to the given directory path.
    ///
    /// The path is normalised (trailing `/` removed except for root `/`).
    /// After every `SAVE_INTERVAL` visits the data is automatically persisted.
    pub fn record_visit(&mut self, path: &str) {
        let normalized = Self::normalize_path(path);

        let now = Self::current_timestamp();

        if let Some(entry) = self.entries.get_mut(&normalized) {
            entry.visit_count += 1;
            entry.last_visited = now;
        } else {
            self.entries.insert(
                normalized.clone(),
                DirEntry {
                    path: normalized,
                    visit_count: 1,
                    last_visited: now,
                    alias: None,
                },
            );
        }

        self.dirty = true;
        self.save_counter += 1;

        if self.save_counter >= SAVE_INTERVAL {
            self.prune();
            let _ = self.persist();
            self.save_counter = 0;
        }
    }

    /// Get the top `limit` directories sorted by visit count (descending).
    pub fn get_frequent(&self, limit: usize) -> Vec<&DirEntry> {
        let mut refs: Vec<&DirEntry> = self.entries.values().collect();
        refs.sort_by(|a, b| b.visit_count.cmp(&a.visit_count));
        refs.truncate(limit);
        refs
    }

    /// Get the `limit` most recently visited directories.
    pub fn get_recent(&self, limit: usize) -> Vec<&DirEntry> {
        let mut refs: Vec<&DirEntry> = self.entries.values().collect();
        refs.sort_by(|a, b| b.last_visited.cmp(&a.last_visited));
        refs.truncate(limit);
        refs
    }

    /// Case-insensitive prefix search over directory paths.
    pub fn search(&self, query: &str) -> Vec<&DirEntry> {
        let lower = query.to_lowercase();
        self.entries
            .values()
            .filter(|e| e.path.to_lowercase().starts_with(&lower))
            .collect()
    }

    /// Set a user-defined alias for a directory.
    pub fn set_alias(&mut self, path: &str, alias: &str) {
        let normalized = Self::normalize_path(path);
        if let Some(entry) = self.entries.get_mut(&normalized) {
            entry.alias = Some(alias.to_string());
            self.dirty = true;
        }
    }

    /// Remove a directory entry, returning it if it existed.
    pub fn remove(&mut self, path: &str) -> Option<DirEntry> {
        let normalized = Self::normalize_path(path);
        let removed = self.entries.remove(&normalized);
        if removed.is_some() {
            self.dirty = true;
        }
        removed
    }

    /// Persist the current entries to the JSON file at `persist_path`.
    pub fn persist(&self) -> Result<(), std::io::Error> {
        if !self.dirty {
            return Ok(());
        }

        // Ensure the parent directory exists.
        if let Some(parent) = self.persist_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let entries: Vec<&DirEntry> = self.entries.values().collect();
        let json = serde_json::to_string_pretty(&entries)?;
        std::fs::write(&self.persist_path, json)?;
        Ok(())
    }

    /// Remove the least-visited entries to stay under `max_entries`.
    pub fn prune(&mut self) {
        if self.entries.len() <= self.max_entries {
            return;
        }

        let excess = self.entries.len() - self.max_entries;

        // Collect paths sorted by visit count ascending (least-visited first).
        let mut sorted: Vec<String> = self
            .entries
            .values()
            .map(|e| e.path.clone())
            .collect();
        sorted.sort_by(|a, b| {
            let va = self.entries[a].visit_count;
            let vb = self.entries[b].visit_count;
            va.cmp(&vb)
        });

        // Take only as many as we need to remove.
        sorted.truncate(excess);

        for path in sorted {
            self.entries.remove(&path);
        }

        self.dirty = true;
    }

    // ---- helpers ----

    /// Default file path for persistence: `~/.config/pureterm/dirfreq.json`.
    fn default_persist_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("pureterm")
            .join("dirfreq.json")
    }

    /// Load entries from a JSON file on disk.
    fn load(path: &PathBuf) -> Option<HashMap<String, DirEntry>> {
        let data = std::fs::read_to_string(path).ok()?;
        let entries: Vec<DirEntry> = serde_json::from_str(&data).ok()?;
        let mut map = HashMap::new();
        for entry in entries {
            map.insert(entry.path.clone(), entry);
        }
        Some(map)
    }

    /// Normalize a path: remove trailing `/` unless the path is exactly `/`.
    fn normalize_path(path: &str) -> String {
        if path.len() > 1 && path.ends_with('/') {
            path.trim_end_matches('/').to_string()
        } else {
            path.to_string()
        }
    }

    /// Current unix timestamp in seconds.
    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_strips_trailing_slash() {
        assert_eq!(DirFrequencyService::normalize_path("/home/user/"), "/home/user");
        assert_eq!(DirFrequencyService::normalize_path("/home/user"), "/home/user");
        assert_eq!(DirFrequencyService::normalize_path("/"), "/");
    }

    #[test]
    fn record_visit_increments_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("freq.json");
        let mut svc = DirFrequencyService::with_persist_path(path);

        svc.record_visit("/home/user/projects");
        svc.record_visit("/home/user/projects");
        svc.record_visit("/home/user/docs");

        let frequent = svc.get_frequent(1);
        assert_eq!(frequent.len(), 1);
        assert_eq!(frequent[0].path, "/home/user/projects");
        assert_eq!(frequent[0].visit_count, 2);
    }

    #[test]
    fn get_recent_returns_latest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("freq.json");
        let mut svc = DirFrequencyService::with_persist_path(path);

        svc.record_visit("/a");

        // Manually inject a future timestamp for /b so the ordering is deterministic
        // even when both visits land within the same second.
        svc.record_visit("/b");
        if let Some(entry) = svc.entries.get_mut("/b") {
            entry.last_visited += 60; // 60 seconds in the future
        }

        let recent = svc.get_recent(1);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].path, "/b");
    }

    #[test]
    fn search_is_case_insensitive_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("freq.json");
        let mut svc = DirFrequencyService::with_persist_path(path);

        svc.record_visit("/Home/User/projects");
        svc.record_visit("/home/user/docs");
        svc.record_visit("/var/log");

        let results = svc.search("/home/user");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn set_and_remove_alias() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("freq.json");
        let mut svc = DirFrequencyService::with_persist_path(path);

        svc.record_visit("/home/user/projects");
        svc.set_alias("/home/user/projects", "proj");

        let entry = svc.entries.get("/home/user/projects").unwrap();
        assert_eq!(entry.alias.as_deref(), Some("proj"));
    }

    #[test]
    fn remove_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("freq.json");
        let mut svc = DirFrequencyService::with_persist_path(path);

        svc.record_visit("/tmp/a");
        assert!(svc.entries.contains_key("/tmp/a"));

        let removed = svc.remove("/tmp/a");
        assert!(removed.is_some());
        assert!(!svc.entries.contains_key("/tmp/a"));

        let removed_again = svc.remove("/tmp/a");
        assert!(removed_again.is_none());
    }

    #[test]
    fn persist_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("freq.json");
        {
            let mut svc = DirFrequencyService::with_persist_path(path.clone());
            svc.record_visit("/home/user/projects");
            svc.record_visit("/home/user/projects");
            // Force persist (dirty flag).
            svc.dirty = true;
            svc.persist().unwrap();
        }

        let svc = DirFrequencyService::with_persist_path(path);
        let entry = svc.entries.get("/home/user/projects").unwrap();
        assert_eq!(entry.visit_count, 2);
    }

    #[test]
    fn prune_removes_least_visited() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("freq.json");
        let mut svc = DirFrequencyService::with_persist_path(path);
        svc.max_entries = 3;

        svc.record_visit("/a"); // 1 visit
        svc.record_visit("/b");
        svc.record_visit("/b"); // 2 visits
        svc.record_visit("/c");
        svc.record_visit("/c");
        svc.record_visit("/c"); // 3 visits
        svc.record_visit("/d");
        svc.record_visit("/d");
        svc.record_visit("/d");
        svc.record_visit("/d"); // 4 visits -- 10th call triggers auto-prune

        // Auto-prune on the 10th visit already removed the least-visited entry.
        assert_eq!(svc.entries.len(), 3);
        assert!(!svc.entries.contains_key("/a")); // least visited removed
    }

    #[test]
    fn default_persist_path_points_to_config_dir() {
        let path = DirFrequencyService::default_persist_path();
        assert!(path.to_string_lossy().contains("pureterm"));
        assert!(path.to_string_lossy().contains("dirfreq.json"));
    }
}
