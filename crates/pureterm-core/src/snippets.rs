//! Saved text snippets the user can configure and insert into the terminal.
//!
//! Modeled on iTerm2's `sources/Snippets/`. The [`SnippetService`] is a pure
//! library type — UI integration (Command Palette, send-to-pane) is wired up
//! by `pureterm-ui` in a follow-up.

use serde::{Deserialize, Serialize};

/// A user-defined text snippet that can be inserted into a running shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// Stable identifier (e.g. UUID or slug). Used for lookup and removal.
    pub id: String,
    /// Human-readable label shown in pickers.
    pub name: String,
    /// The literal text to send to the pane when invoked.
    pub text: String,
    /// Free-form tags used for filtering / search.
    pub tags: Vec<String>,
}

/// In-memory store of snippets with simple search and CRUD operations.
#[derive(Debug, Default, Clone)]
pub struct SnippetService {
    snippets: Vec<Snippet>,
}

impl SnippetService {
    /// Create an empty service.
    pub fn new() -> Self {
        Self { snippets: Vec::new() }
    }

    /// Build a service preloaded with the given snippets (e.g. from config).
    pub fn with_snippets(snippets: Vec<Snippet>) -> Self {
        Self { snippets }
    }

    /// Add a snippet. If a snippet with the same id already exists it is
    /// replaced in place to preserve ordering.
    pub fn add(&mut self, snippet: Snippet) {
        if let Some(existing) = self.snippets.iter_mut().find(|s| s.id == snippet.id) {
            *existing = snippet;
        } else {
            self.snippets.push(snippet);
        }
    }

    /// Remove a snippet by id, returning the removed entry if present.
    pub fn remove(&mut self, id: &str) -> Option<Snippet> {
        let idx = self.snippets.iter().position(|s| s.id == id)?;
        Some(self.snippets.remove(idx))
    }

    /// All snippets in insertion order.
    pub fn list(&self) -> &[Snippet] {
        &self.snippets
    }

    /// Look up a snippet by id.
    pub fn get(&self, id: &str) -> Option<&Snippet> {
        self.snippets.iter().find(|s| s.id == id)
    }

    /// Case-insensitive substring search across `name` and `tags`.
    /// An empty query returns every snippet.
    pub fn find(&self, query: &str) -> Vec<&Snippet> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return self.snippets.iter().collect();
        }
        self.snippets
            .iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&needle)
                    || s.tags.iter().any(|t| t.to_lowercase().contains(&needle))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snip(id: &str, name: &str, text: &str, tags: &[&str]) -> Snippet {
        Snippet {
            id: id.into(),
            name: name.into(),
            text: text.into(),
            tags: tags.iter().map(|t| (*t).into()).collect(),
        }
    }

    #[test]
    fn new_service_is_empty() {
        let svc = SnippetService::new();
        assert!(svc.list().is_empty());
        assert!(svc.find("anything").is_empty());
    }

    #[test]
    fn add_and_get() {
        let mut svc = SnippetService::new();
        let s = snip("a", "List", "ls -la", &["fs"]);
        svc.add(s.clone());
        assert_eq!(svc.list().len(), 1);
        assert_eq!(svc.get("a"), Some(&s));
        assert_eq!(svc.get("missing"), None);
    }

    #[test]
    fn remove_returns_snippet() {
        let mut svc = SnippetService::new();
        let s = snip("a", "List", "ls", &[]);
        svc.add(s.clone());
        assert_eq!(svc.remove("a"), Some(s));
        assert!(svc.list().is_empty());
        assert_eq!(svc.remove("a"), None);
    }

    #[test]
    fn find_matches_name_case_insensitive() {
        let mut svc = SnippetService::new();
        svc.add(snip("a", "Deploy Prod", "make deploy", &[]));
        svc.add(snip("b", "Run Tests", "cargo test", &[]));
        let hits = svc.find("DEPLOY");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "a");
    }

    #[test]
    fn find_matches_tag() {
        let mut svc = SnippetService::new();
        svc.add(snip("a", "Deploy", "make deploy", &["ops", "prod"]));
        svc.add(snip("b", "Tests", "cargo test", &["dev"]));
        let hits = svc.find("OPS");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "a");

        // Empty query returns everything.
        assert_eq!(svc.find("").len(), 2);
        // No match returns empty.
        assert!(svc.find("nope").is_empty());
    }

    #[test]
    fn serde_roundtrip_snippet() {
        let s = snip("x", "Hello", "echo hi", &["greet", "demo"]);
        let json = serde_json::to_string(&s).expect("serialize");
        let back: Snippet = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(s, back);
    }

    #[test]
    fn add_replaces_existing_id() {
        let mut svc = SnippetService::new();
        svc.add(snip("a", "One", "1", &[]));
        svc.add(snip("a", "Updated", "2", &["t"]));
        assert_eq!(svc.list().len(), 1);
        assert_eq!(svc.get("a").unwrap().name, "Updated");
    }
}
