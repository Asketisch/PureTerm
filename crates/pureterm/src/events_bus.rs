//! GAP-6.7: the in-process event hub.
//!
//! [`EventBus`] is the single sink every producer calls. Each [`EventBus::emit`]
//! does two things: (1) append the event to `events.jsonl` (via
//! [`events_log`](crate::events_log), with 16 MiB rotation), and (2) broadcast
//! it to in-process subscribers over a `tokio::sync::broadcast` channel whose
//! capacity ([`EVENT_BUFFER`]) is the in-memory replay window for late
//! subscribers.
//!
//! The GUI owns one `Arc<EventBus>`; the hook shim and external CLI never touch
//! it (they reach events via the file). `emit` is `&self` (interior mutability)
//! so it stays callable when a future off-thread producer appears.
//!
//! Real-time external consumption is file tailing (`pureterm events tail`), not
//! streaming IPC — see [`events_log::tail_follow`](crate::events_log::tail_follow).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::broadcast;

use crate::events::{AppEvent, Category};
use crate::events_log;

/// Broadcast capacity = in-memory replay window for late subscribers.
pub const EVENT_BUFFER: usize = 4096;

pub struct EventBus {
    tx: broadcast::Sender<AppEvent>,
    seq: AtomicU64,
    /// Guards the append+rotate critical section against off-thread producers.
    /// Today every emit runs on the iced main loop, so this is uncontended, but
    /// the lock keeps `emit` correct by construction if that changes.
    log_lock: Mutex<()>,
    path: PathBuf,
}

impl EventBus {
    /// Build a bus that writes to `path`. Wrap in `Arc` for sharing across the
    /// app + iced Subscription.
    pub fn new(path: PathBuf) -> Arc<Self> {
        let (tx, _) = broadcast::channel(EVENT_BUFFER);
        Arc::new(Self {
            tx,
            seq: AtomicU64::new(0),
            log_lock: Mutex::new(()),
            path,
        })
    }

    /// The log path this bus writes to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Subscribe to the live in-process stream. Late subscribers only get the
    /// most recent [`EVENT_BUFFER`] events; a receiver that lags past capacity
    /// gets [`broadcast::error::RecvError::Lagged`] (consumers must tolerate it).
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.tx.subscribe()
    }

    /// Record one event: assigns a monotonic `seq` + timestamp, persists to the
    /// JSONL log, broadcasts to subscribers. Returns the assigned `seq`.
    ///
    /// A send with no subscribers returns `Err` — that's fine, the file is the
    /// source of truth and the CLI tails it.
    pub fn emit(
        &self,
        cat: Category,
        kind: impl Into<String>,
        session_id: Option<String>,
        data: Value,
    ) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let event = AppEvent {
            seq,
            ts: now_millis(),
            cat,
            kind: kind.into(),
            session_id,
            data,
        };
        {
            // Poison-resilient: if a prior `emit` panicked while holding this
            // std Mutex (the critical section swallows its own I/O errors, so a
            // poison is unlikely), recover the guard instead of panicking on the
            // iced main thread (which would kill all future notifications).
            let _guard = self.log_lock.lock().unwrap_or_else(|e| e.into_inner());
            events_log::append_to_path(&self.path, &event);
        }
        let _ = self.tx.send(event);
        seq
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bus_in(dir: &tempfile::TempDir) -> Arc<EventBus> {
        EventBus::new(dir.path().join("events.jsonl"))
    }

    #[test]
    fn emit_assigns_monotonic_seq() {
        let dir = tempfile::tempdir().unwrap();
        let bus = bus_in(&dir);
        let s1 = bus.emit(Category::App, "app.started", None, Value::Null);
        let s2 = bus.emit(Category::App, "app.tick", None, Value::Null);
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert!(s2 > s1);
    }

    #[test]
    fn emit_persists_to_log() {
        let dir = tempfile::tempdir().unwrap();
        let bus = bus_in(&dir);
        bus.emit(Category::Feed, "feed.requested", Some("s1".into()), json!({"tool": "Bash"}));
        let entries = events_log::read_recent_from_path(
            bus.path(),
            &crate::events::TypeFilter::default(),
            10,
            None,
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cat, Category::Feed);
        assert_eq!(entries[0].kind, "feed.requested");
        assert_eq!(entries[0].session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn emit_with_no_subscriber_still_persists() {
        // No subscribe() call — broadcast::send returns Err; emit must not panic.
        let dir = tempfile::tempdir().unwrap();
        let bus = bus_in(&dir);
        bus.emit(Category::App, "app.started", None, Value::Null);
        bus.emit(Category::App, "app.tick", None, Value::Null);
        let entries = events_log::read_recent_from_path(
            bus.path(),
            &crate::events::TypeFilter::default(),
            10,
            None,
        );
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn subscriber_receives_emitted_events() {
        let dir = tempfile::tempdir().unwrap();
        let bus = bus_in(&dir);
        let mut rx = bus.subscribe();
        bus.emit(Category::App, "app.started", None, Value::Null);
        let ev = rx.try_recv().expect("subscriber should receive");
        assert_eq!(ev.cat, Category::App);
        assert_eq!(ev.kind, "app.started");
    }
}
