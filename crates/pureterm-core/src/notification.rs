//! PureTerm 的平台通知抽象。

use std::fmt;

/// 通知的优先级级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Critical,
}

/// 显示给用户的通知。
#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub priority: NotificationPriority,
    pub category: NotificationCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationCategory {
    /// 长时间运行的命令完成。
    CommandFinished,
    /// 终端中触发了响铃。
    Bell,
    /// 触发器匹配。
    Trigger,
    /// 文件传输完成。
    Transfer,
    /// 系统事件。
    System,
    /// 通用。
    General,
}

impl fmt::Display for Notification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}: {}", self.category, self.title, self.body)
    }
}

/// 平台特定通知调度的 trait。
pub trait NotificationService: Send + Sync {
    /// 向用户显示通知。
    fn notify(&self, notification: &Notification);

    /// 检查此平台上是否支持通知。
    fn is_supported(&self) -> bool;
}

/// 使用终端响铃/日志输出的默认通知服务。
pub struct LogNotificationService;

impl NotificationService for LogNotificationService {
    fn notify(&self, notification: &Notification) {
        log::info!("Notification: {}", notification);
    }

    fn is_supported(&self) -> bool {
        true
    }
}

/// 使用 osascript 的 macOS 通知服务。
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
