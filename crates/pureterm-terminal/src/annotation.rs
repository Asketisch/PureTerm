//! Line-anchored terminal annotations.
//!
//! Allows attaching short text notes to specific terminal lines, keyed by
//! Grid `line_id`. Modeled on iTerm2's `PTYAnnotation.m`. The store is
//! independent of the Grid — it only references `line_id`s, so panes can
//! own a store and let the UI render markers near annotated lines.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single line-anchored annotation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Annotation {
    pub id: u64,
    /// Links to a Grid `line_id`.
    pub line_id: u64,
    pub col_start: usize,
    pub col_end: usize,
    pub text: String,
    /// Unix timestamp (seconds) at creation.
    pub created_unix: i64,
}

/// Stores annotations grouped by `line_id` for fast per-line lookup.
#[derive(Default, Debug)]
pub struct AnnotationStore {
    next_id: u64,
    by_line: BTreeMap<u64, Vec<Annotation>>,
}

impl AnnotationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new annotation, returning its freshly-allocated id.
    pub fn add(
        &mut self,
        line_id: u64,
        col_start: usize,
        col_end: usize,
        text: String,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        let created_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let ann = Annotation {
            id,
            line_id,
            col_start,
            col_end,
            text,
            created_unix,
        };
        self.by_line.entry(line_id).or_default().push(ann);
        id
    }

    /// Remove an annotation by id. Returns `true` if it was present.
    pub fn remove(&mut self, id: u64) -> bool {
        let mut removed_line: Option<u64> = None;
        let mut found = false;
        for (line_id, anns) in self.by_line.iter_mut() {
            if let Some(pos) = anns.iter().position(|a| a.id == id) {
                anns.remove(pos);
                if anns.is_empty() {
                    removed_line = Some(*line_id);
                }
                found = true;
                break;
            }
        }
        if let Some(line_id) = removed_line {
            self.by_line.remove(&line_id);
        }
        found
    }

    /// Lookup an annotation by id.
    pub fn get(&self, id: u64) -> Option<&Annotation> {
        self.by_line
            .values()
            .flat_map(|v| v.iter())
            .find(|a| a.id == id)
    }

    /// Annotations attached to `line_id`. Returns an empty slice when none.
    pub fn for_line(&self, line_id: u64) -> &[Annotation] {
        self.by_line
            .get(&line_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// All annotations across all lines.
    pub fn list(&self) -> Vec<&Annotation> {
        self.by_line.values().flat_map(|v| v.iter()).collect()
    }

    /// Update annotation text. Returns `true` if the id existed.
    pub fn update_text(&mut self, id: u64, new_text: String) -> bool {
        for anns in self.by_line.values_mut() {
            if let Some(a) = anns.iter_mut().find(|a| a.id == id) {
                a.text = new_text;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_returns_unique_ids() {
        let mut store = AnnotationStore::new();
        let id1 = store.add(10, 0, 5, "first".into());
        let id2 = store.add(10, 6, 10, "second".into());
        let id3 = store.add(11, 0, 3, "third".into());
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn for_line_returns_only_matching() {
        let mut store = AnnotationStore::new();
        store.add(10, 0, 5, "a".into());
        store.add(10, 6, 8, "b".into());
        store.add(20, 0, 2, "c".into());
        let l10 = store.for_line(10);
        assert_eq!(l10.len(), 2);
        assert!(l10.iter().all(|a| a.line_id == 10));
        let l20 = store.for_line(20);
        assert_eq!(l20.len(), 1);
        assert_eq!(l20[0].text, "c");
        assert!(store.for_line(999).is_empty());
    }

    #[test]
    fn remove_existing_returns_true() {
        let mut store = AnnotationStore::new();
        let id = store.add(5, 0, 3, "note".into());
        assert!(store.remove(id));
        assert!(store.get(id).is_none());
        assert!(store.for_line(5).is_empty());
    }

    #[test]
    fn remove_unknown_returns_false() {
        let mut store = AnnotationStore::new();
        store.add(1, 0, 1, "x".into());
        assert!(!store.remove(9999));
    }

    #[test]
    fn update_text_changes_value() {
        let mut store = AnnotationStore::new();
        let id = store.add(7, 0, 4, "old".into());
        assert!(store.update_text(id, "new".into()));
        assert_eq!(store.get(id).unwrap().text, "new");
        assert!(!store.update_text(9999, "missing".into()));
    }

    #[test]
    fn serde_roundtrip_annotation() {
        let ann = Annotation {
            id: 42,
            line_id: 100,
            col_start: 3,
            col_end: 9,
            text: "hello".into(),
            created_unix: 1_700_000_000,
        };
        let json = serde_json::to_string(&ann).expect("serialize");
        let back: Annotation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ann, back);
    }
}
