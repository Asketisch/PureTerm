//! Platform notification abstraction for PureTerm.

use std::fmt;

/// Priority level for notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Critical,
}

/// A notification to display to the user.
#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub priority: NotificationPriority,
    pub category: NotificationCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationCategory {
    /// A long-running command finished.
    CommandFinished,
    /// Bell triggered in terminal.
    Bell,
    /// Trigger matched.
    Trigger,
    /// File transfer complete.
    Transfer,
    /// System event.
    System,
    /// General purpose.
    General,
}

impl fmt::Display for Notification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}: {}", self.category, self.title, self.body)
    }
}

/// Trait for platform-specific notification dispatch.
pub trait NotificationService: Send + Sync {
    /// Show a notification to the user.
    fn notify(&self, notification: &Notification);

    /// Check if notifications are supported on this platform.
    fn is_supported(&self) -> bool;
}

/// Default notification service using terminal bell / log output.
pub struct LogNotificationService;

impl NotificationService for LogNotificationService {
    fn notify(&self, notification: &Notification) {
        log::info!("Notification: {}", notification);
    }

    fn is_supported(&self) -> bool {
        true
    }
}

/// macOS notification service using osascript.
pub struct MacOSNotificationService;

impl MacOSNotificationService {
    pub fn new() -> Self {
        Self
    }
}

impl NotificationService for MacOSNotificationService {
    fn notify(&self, notification: &Notification) {
        let title = &notification.title;
        let body = &notification.body;
        let script = format!(
            "display notification \"{}\" with title \"PureTerm\" subtitle \"{}\"",
            body.replace('"', "\\\""),
            title.replace('"', "\\\""),
        );
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output();
    }

    fn is_supported(&self) -> bool {
        cfg!(target_os = "macos")
    }
}

impl Default for MacOSNotificationService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_display() {
        let n = Notification {
            title: "Test".into(),
            body: "Body text".into(),
            priority: NotificationPriority::Normal,
            category: NotificationCategory::CommandFinished,
        };
        assert!(n.to_string().contains("Test"));
    }

    #[test]
    fn test_log_service() {
        let service = LogNotificationService;
        assert!(service.is_supported());
        service.notify(&Notification {
            title: "Test".into(),
            body: "Hello".into(),
            priority: NotificationPriority::Low,
            category: NotificationCategory::General,
        });
    }
}
