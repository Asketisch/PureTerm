//! GAP-09: Notifications overlay panel — a read-only history of recent
//! notifications (OSC 9/99/777 + bells) with the source pane, source type,
//! occasion, and timestamp. Clicking an entry focuses its source pane.
//!
//! This module owns the panel *state*. The view (`App::view_notifications_panel`)
//! and the `Message` wiring live in `iced_app.rs`, which has the app's private
//! fields (the `NotificationStore`, pane ids, terminal contexts) in scope.
//!
//! The panel reads the in-memory `NotificationStore` live each frame, so the
//! state here is just the open flag — no async fetch, no staleness, no snapshot.

/// State for the Notifications overlay panel.
#[derive(Debug, Clone, Default)]
pub struct NotificationPanelState {
    /// Panel visibility.
    pub open: bool,
}

impl NotificationPanelState {
    /// Fresh closed panel (used at app startup).
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle visibility (the ⌘⇧M action).
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Close the panel.
    pub fn close(&mut self) {
        self.open = false;
    }
}
