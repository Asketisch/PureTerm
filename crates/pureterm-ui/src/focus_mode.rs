//! Focus mode — hide all UI chrome except the terminal content.

use egui::Ui;

/// Manages focus mode state.
pub struct FocusMode {
    pub active: bool,
    /// Remember what was visible before entering focus mode.
    was_tab_bar_visible: bool,
    was_title_bar_visible: bool,
    was_sidebar_visible: bool,
}

impl Default for FocusMode {
    fn default() -> Self {
        Self {
            active: false,
            was_tab_bar_visible: true,
            was_title_bar_visible: true,
            was_sidebar_visible: false,
        }
    }
}

impl FocusMode {
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle focus mode on/off.
    pub fn toggle(&mut self, tab_bar_visible: bool, title_bar_visible: bool, sidebar_visible: bool) {
        if !self.active {
            // Entering focus mode — save current state
            self.was_tab_bar_visible = tab_bar_visible;
            self.was_title_bar_visible = title_bar_visible;
            self.was_sidebar_visible = sidebar_visible;
            self.active = true;
        } else {
            // Exiting focus mode
            self.active = false;
        }
    }

    /// Whether the tab bar should be visible.
    pub fn show_tab_bar(&self, original: bool) -> bool {
        if self.active { false } else { original }
    }

    /// Whether the title bar should be visible.
    pub fn show_title_bar(&self, original: bool) -> bool {
        if self.active { false } else { original }
    }

    /// Whether the sidebar should be visible.
    pub fn show_sidebar(&self, original: bool) -> bool {
        if self.active { false } else { original }
    }

    /// Whether to show the command palette toggle hint in focus mode.
    pub fn show_focus_hint(&self) -> bool {
        self.active
    }

    /// Restore state (call after exiting focus mode).
    pub fn restore_tab_bar(&self) -> bool {
        self.was_tab_bar_visible
    }

    pub fn restore_title_bar(&self) -> bool {
        self.was_title_bar_visible
    }

    pub fn restore_sidebar(&self) -> bool {
        self.was_sidebar_visible
    }
}
