//! System tray integration for PureTerm.

/// System tray manager.
/// On macOS: Uses NSStatusItem via objc messaging.
/// On Linux: Uses StatusNotifierItem via D-Bus.
/// On Windows: Uses Shell_NotifyIcon.
pub struct SystemTray {
    visible: bool,
    tooltip: String,
}

impl SystemTray {
    pub fn new() -> Self {
        Self {
            visible: false,
            tooltip: "PureTerm".to_string(),
        }
    }

    /// Show the system tray icon.
    pub fn show(&mut self) {
        self.visible = true;
        log::info!("System tray icon shown");
        // Platform-specific tray creation would go here
    }

    /// Hide the system tray icon.
    pub fn hide(&mut self) {
        self.visible = false;
        log::info!("System tray icon hidden");
    }

    /// Set the tray tooltip.
    pub fn set_tooltip(&mut self, tooltip: &str) {
        self.tooltip = tooltip.to_string();
    }

    /// Whether the tray icon is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Handle tray events. Called from the event loop.
    pub fn handle_event(&mut self, event: TrayEvent) -> TrayAction {
        match event {
            TrayEvent::Click => TrayAction::ToggleWindow,
            TrayEvent::RightClick => TrayAction::ShowMenu,
            TrayEvent::DoubleClick => TrayAction::OpenNewTab,
        }
    }
}

impl Default for SystemTray {
    fn default() -> Self {
        Self::new()
    }
}

/// Events from the system tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Click,
    RightClick,
    DoubleClick,
}

/// Actions triggered by tray events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ToggleWindow,
    ShowMenu,
    OpenNewTab,
    Quit,
}
