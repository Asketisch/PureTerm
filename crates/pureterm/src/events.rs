//! GAP-6.7: unified event schema.
//!
//! Every state transition the GUI cares about is recorded as an [`AppEvent`]:
//! tab/pane lifecycle, config reload, feed approvals, agent sessions, PTY
//! spawn/exit, etc. The [`EventBus`](crate::events_bus::EventBus) persists each
//! event to `events.jsonl` and broadcasts it to in-process subscribers; the CLI
//! (`pureterm events`) tails the file for real-time consumption.
//!
//! This module is the stable public schema only — no I/O, no bus logic.

use serde_json::Value;

/// The 12 event categories. Serialized lowercase (`Feed` → `"feed"`).
///
/// `Session` carries PTY/daemon-session lifecycle (spawned/exited/reattached/
/// hibernated/resumed) as a first-class concern, rather than folding it under
/// `Surface` — which GAP-2.1 will redefine, so aliasing now would force a rename.
/// `Notification`/`Sidebar` are reserved: their producers land with future
/// GAPs (Notification System / programmable sidebar). `Workspace`/`Browser`
/// are produced by GAP-2.1 / GAP-3.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Window,
    Workspace,
    Surface,
    Pane,
    Session,
    Notification,
    Feed,
    Agent,
    Sidebar,
    Browser,
    App,
    Config,
}

impl Category {
    /// Lowercase wire name (matches the serde representation), used for the
    /// `--type` category-prefix filter without going through serialize.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Window => "window",
            Category::Workspace => "workspace",
            Category::Surface => "surface",
            Category::Pane => "pane",
            Category::Session => "session",
            Category::Notification => "notification",
            Category::Feed => "feed",
            Category::Agent => "agent",
            Category::Sidebar => "sidebar",
            Category::Browser => "browser",
            Category::App => "app",
            Category::Config => "config",
        }
    }
}

/// One structured event. Serialized one-per-line into `events.jsonl`.
///
/// Field order is fixed for human-readability of the raw log: `seq` (monotonic
/// id), `ts` (epoch millis), `cat` (category), `kind` (`"<cat>.<verb>"`),
/// `session_id` (optional), `data` (arbitrary payload).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppEvent {
    pub seq: u64,
    pub ts: u64,
    pub cat: Category,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

impl AppEvent {
    /// Construct an event with the given kind + payload; `seq`/`ts` are filled
    /// by [`EventBus::emit`](crate::events_bus::EventBus::emit).
    pub fn new(cat: Category, kind: impl Into<String>, session_id: Option<String>, data: Value) -> Self {
        Self {
            seq: 0,
            ts: 0,
            cat,
            kind: kind.into(),
            session_id,
            data,
        }
    }
}

/// A parsed `--type` filter. `--type feed` → category only; `--type feed.resolved`
/// → category + exact kind. Empty filter (no `--type`) matches everything.
#[derive(Debug, Clone, Default)]
pub struct TypeFilter {
    pub category: Option<String>,
    pub kind: Option<String>,
}

impl TypeFilter {
    /// Parse a `--type` argument. `None` or empty → match all.
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::default();
        };
        match raw.split_once('.') {
            Some((cat, kind)) if !kind.is_empty() => Self {
                category: Some(cat.to_string()),
                kind: Some(kind.to_string()),
            },
            _ => Self {
                category: Some(raw.to_string()),
                kind: None,
            },
        }
    }

    /// Does this event pass the filter?
    pub fn matches(&self, event: &AppEvent) -> bool {
        if let Some(cat) = &self.category {
            if event.cat.as_str() != cat.as_str() {
                return false;
            }
        }
        if let Some(kind) = &self.kind {
            if !event.kind.ends_with(kind.as_str()) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(cat: Category, kind: &str) -> AppEvent {
        AppEvent::new(cat, kind, None, Value::Null)
    }

    #[test]
    fn category_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Category::Feed).unwrap(),
            "\"feed\""
        );
        assert_eq!(Category::Session.as_str(), "session");
    }

    #[test]
    fn app_event_round_trips_through_jsonl() {
        let mut e = ev(Category::Feed, "feed.requested");
        e.seq = 7;
        e.ts = 1719000000000;
        e.session_id = Some("abc".into());
        e.data = serde_json::json!({"tool": "Bash"});
        let line = serde_json::to_string(&e).unwrap();
        let back: AppEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(back.seq, 7);
        assert_eq!(back.cat, Category::Feed);
        assert_eq!(back.kind, "feed.requested");
        assert_eq!(back.session_id.as_deref(), Some("abc"));
        assert_eq!(back.data["tool"], "Bash");
    }

    #[test]
    fn app_event_omits_null_data_and_none_session() {
        let e = ev(Category::App, "app.started");
        let line = serde_json::to_string(&e).unwrap();
        assert!(!line.contains("session_id"));
        assert!(!line.contains("data"));
    }

    #[test]
    fn type_filter_category_prefix_matches_namespace() {
        let f = TypeFilter::parse(Some("feed"));
        assert!(f.matches(&ev(Category::Feed, "feed.requested")));
        assert!(f.matches(&ev(Category::Feed, "feed.resolved")));
        assert!(!f.matches(&ev(Category::App, "app.started")));
    }

    #[test]
    fn type_filter_dotted_matches_exact_kind_suffix() {
        let f = TypeFilter::parse(Some("feed.resolved"));
        assert!(f.matches(&ev(Category::Feed, "feed.resolved")));
        assert!(!f.matches(&ev(Category::Feed, "feed.requested")));
    }

    #[test]
    fn type_filter_empty_matches_all() {
        let f = TypeFilter::parse(None);
        assert!(f.matches(&ev(Category::App, "app.started")));
        let f2 = TypeFilter::parse(Some(""));
        assert!(f2.matches(&ev(Category::App, "app.started")));
    }
}
