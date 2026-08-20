//! GAP-57:MacBook 触感反馈。
//!
//! macOS 上 `NSHapticFeedbackManager.defaultPerformer().performFeedbackPattern(_:performanceTime:)`
//! 触发系统触控板震动马达。错误(非 Mac、无力度触控
//! 触控板、API 拒绝请求)只记日志,其余忽略
//! —— 触感反馈尽力而为,绝不阻塞 UI 线程。
//!
//! 其它平台:空操作(系统没有等价的 API)。

/// 触感事件类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HapticEvent {
    TabSwitch,
    Error,
    Notification,
}

/// `[haptic]` 配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HapticConfig {
    pub enabled: bool,
    pub on_tab_switch: bool,
    pub on_error: bool,
    pub on_notification: bool,
}

impl Default for HapticConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            on_tab_switch: true,
            on_error: true,
            on_notification: true,
        }
    }
}

impl HapticConfig {
    pub fn should_fire(&self, event: HapticEvent) -> bool {
        if !self.enabled {
            return false;
        }
        match event {
            HapticEvent::TabSwitch => self.on_tab_switch,
            HapticEvent::Error => self.on_error,
            HapticEvent::Notification => self.on_notification,
        }
    }
}

/// 触感请求的结果。`Fired` 表示 API 已受理
/// 请求;`Skipped` / `Failed` 区分用户禁用与平台
/// 拒绝(非 Mac 上多为前者;缺少力度触控的 Mac 上
/// 多为后者)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HapticOutcome {
    Skipped,
    Fired,
    Failed(String),
}

#[cfg(target_os = "macos")]
fn pattern_for(event: HapticEvent) -> objc2_app_kit::NSHapticFeedbackPattern {
    use objc2_app_kit::NSHapticFeedbackPattern;
    match event {
        // Generic 是最稳妥的选择:alignment / level / generic 都
        // 映射到同一底层马达,而 `Generic` 在
        // 每台支持触感的 Mac(10.11+)上都可用。
        HapticEvent::TabSwitch => NSHapticFeedbackPattern::Generic,
        HapticEvent::Error => NSHapticFeedbackPattern::Generic,
        HapticEvent::Notification => NSHapticFeedbackPattern::Generic,
    }
}

/// 执行一次触感事件。可在任意线程调用(macOS API 被
/// 文档声明为线程安全)。返回结果,供调用方(和测试)
/// 区分跳过、触发与错误。
pub fn perform(cfg: &HapticConfig, event: HapticEvent) -> HapticOutcome {
    if !cfg.should_fire(event) {
        return HapticOutcome::Skipped;
    }
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::ProtocolObject;
                use objc2_app_kit::{NSHapticFeedbackManager, NSHapticFeedbackPerformanceTime};
        let performer =
            NSHapticFeedbackManager::defaultPerformer();
        let performer: &ProtocolObject<dyn objc2_app_kit::NSHapticFeedbackPerformer> = &*performer;
        let pattern = pattern_for(event);
        let time = NSHapticFeedbackPerformanceTime::Default;
        // performFeedbackPattern:performanceTime: 返回 void。我们
        // 直接调用 trait 方法(objc2 0.6 在
        // ProtocolObject 上重新导出)并置于 `catch_unwind` 边界内,
        // 使行为异常的 performer 无法崩溃 UI 线程。
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            objc2_app_kit::NSHapticFeedbackPerformer::performFeedbackPattern_performanceTime(
                &*performer, pattern, time,
            );
        }));
        match res {
            Ok(()) => HapticOutcome::Fired,
            Err(_) => HapticOutcome::Failed("performer panicked".into()),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = event;
        HapticOutcome::Skipped
    }
}

/// 便捷方法:发射后不管。以 debug 级别记录结果并
/// 丢弃返回值。
pub fn perform_logged(cfg: &HapticConfig, event: HapticEvent) {
    let outcome = perform(cfg, event);
    match outcome {
        HapticOutcome::Skipped => {}
        HapticOutcome::Fired => log::debug!("haptic {:?} fired", event),
        HapticOutcome::Failed(e) => log::debug!("haptic {:?} failed: {e}", event),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_skips() {
        assert!(!HapticConfig::default().should_fire(HapticEvent::Error));
    }

    #[test]
    fn enabled_by_kind() {
        let cfg = HapticConfig {
            enabled: true,
            on_tab_switch: true,
            on_error: false,
            on_notification: true,
        };
        assert!(cfg.should_fire(HapticEvent::TabSwitch));
        assert!(!cfg.should_fire(HapticEvent::Error));
        assert!(cfg.should_fire(HapticEvent::Notification));
    }

    #[test]
    fn perform_skipped_when_disabled() {
        let cfg = HapticConfig::default();
        assert_eq!(perform(&cfg, HapticEvent::Error), HapticOutcome::Skipped);
    }

    #[test]
    fn perform_logged_is_safe() {
        let cfg = HapticConfig::default();
        // 配置禁用 => Skipped,日志路径为空操作。
        perform_logged(&cfg, HapticEvent::TabSwitch);
    }
}
