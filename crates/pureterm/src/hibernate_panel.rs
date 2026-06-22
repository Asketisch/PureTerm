//! GAP-7.3: hibernation confirmation panel — GUI-side state.
//!
//! When `HibernationScanTick` (iced_app) finds a session satisfying the dual
//! idle condition (PTY silent AND CPU near-zero for the configured threshold)
//! AND the live-session count exceeds `max_live_terminals`, it pushes a
//! `PendingHibernation` here. The right-side pinned overlay shows it with a
//! countdown; the user confirms (→ `agent.hibernate` SIGSTOP) or cancels.
//! Auto-confirm on timeout. Only one confirmation is shown at a time — a busy
//! scan simply re-evaluates on the next tick once the current one resolves.

use std::time::Instant;

/// One pending hibernation confirmation.
#[derive(Debug, Clone)]
pub struct PendingHibernation {
    /// Daemon session id to SIGSTOP on confirm.
    pub session_id: String,
    /// PTY/agent kind label for display ("local" / "claude-code" / ...).
    pub kind: String,
    /// When the confirmation was raised (drives the countdown).
    pub started_at: Instant,
    /// Auto-confirm after this many seconds (from config, default 60).
    pub confirm_secs: u64,
}

/// Hibernation panel state. Always present on the app; visibility =
/// `pending.is_some()`. Mirrors `feed_panel::FeedPanelState`'s shape but holds
/// a single pending confirmation (hibernation is one-at-a-time, unlike Feed's
/// queue) and no approval memory.
pub struct HibernationPanelState {
    pub pending: Option<PendingHibernation>,
}

impl Default for HibernationPanelState {
    fn default() -> Self {
        Self { pending: None }
    }
}

impl HibernationPanelState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the overlay panel should render.
    pub fn is_visible(&self) -> bool {
        self.pending.is_some()
    }

    /// Raise a confirmation. Only one at a time; if one is already pending the
    /// new request is dropped (the scan re-evaluates next tick).
    pub fn request(&mut self, p: PendingHibernation) {
        if self.pending.is_none() {
            self.pending = Some(p);
        }
    }

    /// User confirmed → take the session id to hibernate (clears the panel).
    pub fn confirm(&mut self) -> Option<String> {
        self.pending.take().map(|p| p.session_id)
    }

    /// User cancelled → clear without hibernating.
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// Auto-confirm if the confirmation window elapsed. Returns the session id
    /// to hibernate if the timeout fired.
    pub fn expire_timeout(&mut self) -> Option<String> {
        let p = self.pending.as_ref()?;
        if p.started_at.elapsed().as_secs() >= p.confirm_secs {
            self.pending.take().map(|p| p.session_id)
        } else {
            None
        }
    }

    /// Seconds left in the countdown (for display); `None` when no panel.
    pub fn remaining_secs(&self) -> Option<u64> {
        self.pending
            .as_ref()
            .map(|p| p.confirm_secs.saturating_sub(p.started_at.elapsed().as_secs()))
    }

    /// The pending session id, if any (for display / status bar).
    pub fn focused_session_id(&self) -> Option<&str> {
        self.pending.as_ref().map(|p| p.session_id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn pending(confirm_secs: u64) -> PendingHibernation {
        PendingHibernation {
            session_id: "s1".into(),
            kind: "claude-code".into(),
            started_at: Instant::now(),
            confirm_secs,
        }
    }

    #[test]
    fn request_then_confirm_returns_session_id() {
        let mut st = HibernationPanelState::new();
        assert!(!st.is_visible());
        st.request(pending(60));
        assert!(st.is_visible());
        assert_eq!(st.confirm(), Some("s1".to_string()));
        assert!(!st.is_visible());
    }

    #[test]
    fn cancel_clears_without_hibernating() {
        let mut st = HibernationPanelState::new();
        st.request(pending(60));
        st.cancel();
        assert!(st.confirm().is_none());
    }

    #[test]
    fn second_request_while_pending_is_dropped() {
        let mut st = HibernationPanelState::new();
        st.request(pending(60));
        let mut p2 = pending(60);
        p2.session_id = "s2".into();
        st.request(p2);
        // First pending wins; second is dropped.
        assert_eq!(st.focused_session_id(), Some("s1"));
    }

    #[test]
    fn expire_timeout_fires_after_window() {
        let mut st = HibernationPanelState::new();
        let mut p = pending(60);
        p.started_at = Instant::now() - Duration::from_secs(61);
        st.pending = Some(p);
        assert_eq!(st.expire_timeout(), Some("s1".to_string()));
        assert!(!st.is_visible());
    }

    #[test]
    fn expire_timeout_none_within_window() {
        let mut st = HibernationPanelState::new();
        st.request(pending(60));
        assert!(st.expire_timeout().is_none());
        assert!(st.is_visible());
    }
}
