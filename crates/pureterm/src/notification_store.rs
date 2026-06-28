//! GAP-09: in-memory notification store — the UI source of truth.
//!
//! The `NotificationStore` holds the live, mutable view (history ring buffer,
//! per-pane unread counts, the attention set, bell throttle). The EventBus
//! (`events.jsonl`) remains an append-only audit log: unread state is mutable
//! (read→0 on focus), which an append-only log cannot represent.
//!
//! Time is passed in (`now_ms`) rather than read from the clock, so the store is
//! pure and unit-testable with no `Instant::now()` / global state.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::split_tree::PaneId;

pub use alacritty_terminal::event::{Notification, NotifySource};

/// One notification as recorded in the history (notification + provenance).
#[derive(Debug, Clone)]
pub struct NotificationEntry {
    /// Monotonic store-assigned id (distinct from the kitty OSC 99 `i`).
    pub seq: u64,
    pub notification: Notification,
    pub pane_id: PaneId,
    /// Epoch millis (matches `AppEvent.ts`).
    pub received_at: u64,
}

/// Cheap `Copy` snapshot handed to the status bar / dock badge each frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NotificationSummary {
    /// Total unread across all panes (status-bar count + Dock badge).
    pub total_unread: u32,
    /// True if any pane needs an attention ring.
    pub any_attention: bool,
}

/// Default history capacity (bounded retention for the panel).
pub const DEFAULT_HISTORY_CAP: usize = 200;

#[derive(Debug)]
pub struct NotificationStore {
    history: VecDeque<NotificationEntry>,
    cap: usize,
    unread: HashMap<PaneId, u32>,
    attention: HashSet<PaneId>,
    /// Last bell timestamp per pane (epoch millis) for throttle/coalesce.
    bell_last: HashMap<PaneId, u64>,
    next_seq: u64,
}

impl NotificationStore {
    pub fn new(cap: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(cap.min(1024)),
            cap,
            unread: HashMap::new(),
            attention: HashSet::new(),
            bell_last: HashMap::new(),
            next_seq: 1,
        }
    }

    /// Record a notification.
    ///
    /// Returns `true` when a fresh notification was recorded (the caller may fire
    /// a desktop toast, subject to focus/visibility gating). Returns `false`
    /// when a bell was suppressed by the per-pane throttle (coalesced into the
    /// prior bell — ring pulses, no new entry, no toast).
    pub fn post(
        &mut self,
        mut entry: NotificationEntry,
        now_ms: u64,
        bell_throttle_ms: u64,
    ) -> bool {
        let pane = entry.pane_id;

        // Bell throttle/coalesce: at most one bell-derived notification per
        // `bell_throttle_ms` window per pane. Within the window, suppress the new
        // entry (the prior bell's ring already pulses) and return false so the
        // caller skips the desktop toast. The window resets on each *recorded*
        // bell, not each attempt, so a sustained bell loop yields ~1/period.
        if entry.notification.source == NotifySource::Bell {
            if let Some(&last) = self.bell_last.get(&pane) {
                if now_ms.saturating_sub(last) < bell_throttle_ms {
                    return false;
                }
            }
            self.bell_last.insert(pane, now_ms);
        }

        // OSC 99 same-id replace (dedup safety net for cross-buffer chunks and
        // repeated same-`i` notifications). Content refresh only — does not
        // re-bump unread/attention (those were set on first post).
        if let Some(id) = entry.notification.id.clone() {
            if let Some(existing) = self
                .history
                .iter_mut()
                .find(|e| e.notification.id.as_deref() == Some(id.as_str()))
            {
                let seq = existing.seq;
                *existing = entry;
                existing.seq = seq;
                return true;
            }
        }

        // Append new entry, evict oldest past cap.
        entry.seq = self.next_seq;
        self.next_seq += 1;
        self.history.push_back(entry);
        while self.history.len() > self.cap {
            self.history.pop_front();
        }

        // Unread + attention.
        *self.unread.entry(pane).or_insert(0) += 1;
        self.attention.insert(pane);
        true
    }

    /// Mark a pane's notifications read (focus) — clears its unread count and
    /// removes it from the attention set. History is retained for the panel.
    pub fn clear_unread(&mut self, pane_id: PaneId) {
        self.unread.remove(&pane_id);
        self.attention.remove(&pane_id);
        // Reset the bell throttle on focus so a freshly-focused pane's next bell
        // is not coalesced into a pre-focus burst.
        self.bell_last.remove(&pane_id);
    }

    /// Clear all unread state (e.g. user dismissed everything). History retained.
    pub fn clear_all(&mut self) {
        self.unread.clear();
        self.attention.clear();
        self.bell_last.clear();
    }

    pub fn summary(&self) -> NotificationSummary {
        NotificationSummary {
            total_unread: self.unread.values().sum(),
            any_attention: !self.attention.is_empty(),
        }
    }

    /// GAP-09: whether a pane currently needs an attention ring (has unread
    /// notifications). Used by the per-pane border closure in `view_pane`.
    pub fn needs_attention(&self, pane_id: PaneId) -> bool {
        self.attention.contains(&pane_id)
    }

    pub fn history(&self) -> &VecDeque<NotificationEntry> {
        &self.history
    }

    /// Number of recorded entries (test helper).
    pub fn len(&self) -> usize {
        self.history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::{NotifyOccasion};

    fn notif(source: NotifySource, body: &str) -> Notification {
        Notification {
            title: None,
            body: body.to_string(),
            id: None,
            occasion: NotifyOccasion::Always,
            source,
        }
    }

    fn entry(pane: u64, n: Notification) -> NotificationEntry {
        NotificationEntry { seq: 0, notification: n, pane_id: PaneId(pane), received_at: 0 }
    }

    #[test]
    fn post_increments_unread_and_marks_attention() {
        let mut s = NotificationStore::new(10);
        let posted = s.post(entry(1, notif(NotifySource::Osc9, "hi")), 0, 1000);
        assert!(posted, "OSC 9 should be recorded");
        let sum = s.summary();
        assert_eq!(sum.total_unread, 1);
        assert!(sum.any_attention);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn clear_unread_drops_count_and_attention() {
        let mut s = NotificationStore::new(10);
        s.post(entry(1, notif(NotifySource::Osc9, "a")), 0, 1000);
        s.post(entry(1, notif(NotifySource::Osc9, "b")), 1, 1000);
        assert_eq!(s.summary().total_unread, 2);
        s.clear_unread(PaneId(1));
        let sum = s.summary();
        assert_eq!(sum.total_unread, 0);
        assert!(!sum.any_attention, "attention cleared on focus");
        // History retained for the panel.
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn summary_totals_across_panes() {
        let mut s = NotificationStore::new(10);
        s.post(entry(1, notif(NotifySource::Osc9, "a")), 0, 1000);
        s.post(entry(2, notif(NotifySource::Osc9, "b")), 1, 1000);
        s.post(entry(2, notif(NotifySource::Osc9, "c")), 2, 1000);
        assert_eq!(s.summary().total_unread, 3);
    }

    #[test]
    fn ring_cap_evicts_oldest() {
        let mut s = NotificationStore::new(3);
        for i in 0..5 {
            s.post(entry(1, notif(NotifySource::Osc9, &format!("n{i}"))), i, 1000);
        }
        assert_eq!(s.len(), 3, "capped at 3");
        // Oldest evicted; newest three retained.
        let bodies: Vec<&str> = s.history().iter().map(|e| e.notification.body.as_str()).collect();
        assert_eq!(bodies, vec!["n2", "n3", "n4"]);
    }

    #[test]
    fn bell_throttle_suppresses_rapid_bells() {
        let mut s = NotificationStore::new(10);
        assert!(s.post(entry(1, notif(NotifySource::Bell, "Bell")), 0, 1000), "first bell recorded");
        assert!(!s.post(entry(1, notif(NotifySource::Bell, "Bell")), 500, 1000), "bell within 1s suppressed");
        assert!(!s.post(entry(1, notif(NotifySource::Bell, "Bell")), 999, 1000), "still within window");
        assert!(s.post(entry(1, notif(NotifySource::Bell, "Bell")), 1001, 1000), "after window, recorded again");
        // Two entries total (first + the post-window one); the two within-window were coalesced.
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn bell_throttle_is_per_pane() {
        let mut s = NotificationStore::new(10);
        assert!(s.post(entry(1, notif(NotifySource::Bell, "Bell")), 0, 1000));
        assert!(s.post(entry(2, notif(NotifySource::Bell, "Bell")), 0, 1000), "different pane not throttled");
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn same_id_replaces_entry() {
        // OSC 99 dedup safety net: same `i` replaces the prior entry (kitty update).
        let mut s = NotificationStore::new(10);
        let mut a = notif(NotifySource::Osc99, "first");
        a.id = Some("job1".to_string());
        let mut b = notif(NotifySource::Osc99, "second");
        b.id = Some("job1".to_string());
        s.post(entry(1, a), 0, 1000);
        s.post(entry(1, b), 1, 1000);
        assert_eq!(s.len(), 1, "same id replaced, not appended");
        assert_eq!(s.history()[0].notification.body, "second");
    }

    #[test]
    fn clear_all_resets() {
        let mut s = NotificationStore::new(10);
        s.post(entry(1, notif(NotifySource::Osc9, "a")), 0, 1000);
        s.clear_all();
        assert_eq!(s.summary().total_unread, 0);
        assert!(!s.summary().any_attention);
    }

    #[test]
    fn assigns_monotonic_seq() {
        let mut s = NotificationStore::new(10);
        s.post(entry(1, notif(NotifySource::Osc9, "a")), 0, 1000);
        s.post(entry(1, notif(NotifySource::Osc9, "b")), 1, 1000);
        let seqs: Vec<u64> = s.history().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![1, 2]);
    }
}
