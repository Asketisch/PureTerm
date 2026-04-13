use std::sync::{Arc, Mutex};

use anyhow::Result;

/// Manager for broadcasting input to multiple sessions simultaneously.
///
/// `SyncInputManager` decouples itself from the `Session` trait's `&mut self`
/// requirement by accepting closures that forward data to the underlying
/// sessions through channels or other mechanisms.
pub struct SyncInputManager {
    sessions: Vec<SyncSessionHandle>,
    enabled: bool,
}

/// Handle to a session registered for sync input.
pub struct SyncSessionHandle {
    pub id: u64,
    pub name: String,
    writer: Arc<Mutex<Box<dyn Fn(&[u8]) -> Result<()> + Send>>>,
}

/// Result of a sync input broadcast operation.
pub struct SyncInputResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: Vec<(u64, String)>,
}

impl SyncInputManager {
    /// Create an empty manager with sync mode disabled.
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            enabled: false,
        }
    }

    /// Register a session for sync input.
    ///
    /// The `writer` closure is expected to capture a sender channel or similar
    /// mechanism that forwards data to the actual session, avoiding the need
    /// to hold a mutable reference to the session itself.
    pub fn add_session(
        &mut self,
        id: u64,
        name: String,
        writer: Box<dyn Fn(&[u8]) -> Result<()> + Send>,
    ) {
        if self.sessions.iter().any(|s| s.id == id) {
            return;
        }
        self.sessions.push(SyncSessionHandle {
            id,
            name,
            writer: Arc::new(Mutex::new(writer)),
        });
    }

    /// Remove a previously registered session by its ID.
    pub fn remove_session(&mut self, id: u64) {
        self.sessions.retain(|s| s.id != id);
    }

    /// Broadcast `data` to every registered session.
    ///
    /// Returns a `SyncInputResult` summarising how many sessions received the
    /// data and which ones (if any) failed along with an error description.
    pub fn broadcast(&self, data: &[u8]) -> SyncInputResult {
        let total = self.sessions.len();
        let mut succeeded = 0;
        let mut failed = Vec::new();

        for session in &self.sessions {
            match session.writer.lock() {
                Ok(lock) => match lock(data) {
                    Ok(()) => succeeded += 1,
                    Err(e) => failed.push((session.id, e.to_string())),
                },
                Err(e) => failed.push((session.id, format!("lock poisoned: {e}"))),
            }
        }

        SyncInputResult {
            total,
            succeeded,
            failed,
        }
    }

    /// Enable or disable sync input mode.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Whether sync input mode is currently active.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Return a slice of all registered session handles.
    pub fn sessions(&self) -> &[SyncSessionHandle] {
        &self.sessions
    }

    /// Number of sessions currently registered.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for SyncInputManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn new_manager_is_empty_and_disabled() {
        let mgr = SyncInputManager::new();
        assert!(!mgr.is_enabled());
        assert_eq!(mgr.session_count(), 0);
        assert!(mgr.sessions().is_empty());
    }

    #[test]
    fn default_is_same_as_new() {
        let mgr = SyncInputManager::default();
        assert!(!mgr.is_enabled());
        assert_eq!(mgr.session_count(), 0);
    }

    #[test]
    fn add_and_remove_sessions() {
        let mut mgr = SyncInputManager::new();
        mgr.add_session(1, "s1".into(), Box::new(|_data| Ok(())));
        mgr.add_session(2, "s2".into(), Box::new(|_data| Ok(())));
        assert_eq!(mgr.session_count(), 2);

        // Duplicate ID is ignored.
        mgr.add_session(1, "s1-again".into(), Box::new(|_data| Ok(())));
        assert_eq!(mgr.session_count(), 2);

        mgr.remove_session(1);
        assert_eq!(mgr.session_count(), 1);
        assert_eq!(mgr.sessions()[0].id, 2);
    }

    #[test]
    fn broadcast_delivers_to_all() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut mgr = SyncInputManager::new();

        for i in 0..3 {
            let c = Arc::clone(&counter);
            mgr.add_session(i, format!("s{i}"), Box::new(move |data| {
                assert_eq!(data, b"hello");
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }));
        }

        let result = mgr.broadcast(b"hello");
        assert_eq!(result.total, 3);
        assert_eq!(result.succeeded, 3);
        assert!(result.failed.is_empty());
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn broadcast_collects_failures() {
        let mut mgr = SyncInputManager::new();
        mgr.add_session(10, "ok".into(), Box::new(|_data| Ok(())));
        mgr.add_session(
            20,
            "bad".into(),
            Box::new(|_data| Err(anyhow::anyhow!("write failed"))),
        );

        let result = mgr.broadcast(b"x");
        assert_eq!(result.total, 2);
        assert_eq!(result.succeeded, 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0, 20);
        assert!(result.failed[0].1.contains("write failed"));
    }

    #[test]
    fn enable_disable() {
        let mut mgr = SyncInputManager::new();
        assert!(!mgr.is_enabled());
        mgr.set_enabled(true);
        assert!(mgr.is_enabled());
        mgr.set_enabled(false);
        assert!(!mgr.is_enabled());
    }
}
