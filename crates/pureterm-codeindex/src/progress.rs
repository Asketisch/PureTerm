//! Optional indexing progress counters (status bar / MCP diagnostics).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Shared progress snapshot for background indexing (GUI status-bar stub).
#[derive(Debug, Default)]
pub struct IndexProgress {
    indexed: AtomicU32,
    total: AtomicU32,
    running: AtomicBool,
}

impl IndexProgress {
    pub fn reset(&self, total: u32) {
        self.total.store(total, Ordering::Relaxed);
        self.indexed.store(0, Ordering::Relaxed);
        self.running.store(total > 0, Ordering::Relaxed);
    }

    pub fn inc(&self) {
        self.indexed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn finish(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> (u32, u32) {
        (
            self.indexed.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }

    /// Percent complete 0–100 when `total > 0` and indexing is active.
    pub fn percent(&self) -> Option<u8> {
        if !self.is_running() {
            return None;
        }
        let (done, total) = self.snapshot();
        if total == 0 {
            return None;
        }
        Some(((done as u64 * 100) / total as u64).min(100) as u8)
    }
}
