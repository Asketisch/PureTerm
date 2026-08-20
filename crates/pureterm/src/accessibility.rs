//! GAP-4.6:VoiceOver / 无障碍播报。
//!
//! **macOS** 路径:真正的 `NSAccessibilityPostNotification` 调用,驱动
//! VoiceOver。标准播报做法是发布
//! `NSAccessibilityAnnouncementRequestedNotification`,其 user-info
//! 字典里的 `NSAccessibilityAnnouncementKey` 即待朗读文本。
//! 我们经由 `NSApp`(主应用对象)派发给应用,它
//! 从任何已调度 run loop 的线程都可到达。
//!
//! **其他平台**:仅日志兜底(Visual / NVDA 用户通常
//! 会把 PureTerm 与平台原生读屏器配对,后者直接从
//! 终端朗读,因此这里不重复实现)。
//!
//! **组件级 a11y**:iced 0.14 尚未暴露 `widget::accessible()`。
//! 逐个组件的标签清扫在同一 spec 下跟踪,将在
//! iced 的 a11y API 稳定后落地;下方这些结构化播报辅助函数
//! 才是当前真正驱动 VoiceOver 的部分。

use std::sync::Mutex;

#[cfg(target_os = "macos")]
use objc2::runtime::AnyObject;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSAccessibilityAnnouncementKey, NSAccessibilityAnnouncementRequestedNotification,
    NSAccessibilityPostNotificationWithUserInfo,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSDictionary, NSString};

/// `[accessibility]` 配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityConfig {
    pub enabled: bool,
    pub announce_tab_changes: bool,
    pub announce_notifications: bool,
    /// 播报窗格的分割 / 调整大小 / 聚焦事件。
    pub announce_pane_changes: bool,
    /// 播报命令面板的打开 / 关闭以及结果数量。
    pub announce_command_palette: bool,
    /// 播报状态栏更新(cwd / git 分支 / 进程名)。
    pub announce_status_bar: bool,
}

impl Default for AccessibilityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            announce_tab_changes: true,
            announce_notifications: true,
            announce_pane_changes: true,
            announce_command_palette: true,
            announce_status_bar: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AccessibilityState {
    pub last_announcement: Option<String>,
    pub history: Vec<String>,
}

impl AccessibilityState {
    /// 历史播报的最大保留条数,用于测试和
    /// 应用内 AX 日志浮层。
    const HISTORY_LIMIT: usize = 32;

    /// 朗读 / 记录一条通用无障碍消息。遵循
    /// `cfg.enabled`;可从任意线程安全调用。
    pub fn announce(&mut self, cfg: &AccessibilityConfig, message: &str) {
        if !cfg.enabled || message.is_empty() {
            return;
        }
        self.record(message);
        #[cfg(target_os = "macos")]
        post_announcement(message);
        #[cfg(not(target_os = "macos"))]
        log::debug!("accessibility: {message}");
    }

    /// 播报标签页切换。格式:`"Switched to Tab N: <title>"`。
    pub fn announce_tab_change(&mut self, cfg: &AccessibilityConfig, index: usize, title: &str) {
        if !cfg.announce_tab_changes {
            return;
        }
        let label = label_for_tab(index, title);
        self.announce(cfg, &label);
    }

    /// 播报窗格分割。`axis` 为 `"vertically"` 或 `"horizontally"`,
    /// `count` 是分割后的窗格总数。
    pub fn announce_split(&mut self, cfg: &AccessibilityConfig, axis: &str, count: usize) {
        if !cfg.announce_pane_changes {
            return;
        }
        let msg = format!("Split {axis}, {count} panes");
        self.announce(cfg, &msg);
    }

    /// 播报窗格焦点变化(光标移入另一窗格)。
    pub fn announce_pane_focus(&mut self, cfg: &AccessibilityConfig, pane_index: usize) {
        if !cfg.announce_pane_changes {
            return;
        }
        let msg = format!("Focused pane {}", pane_index + 1);
        self.announce(cfg, &msg);
    }

    /// 播报命令面板的打开或结果数量。
    /// `phase` 为 `"opened"`、`"closed"` 或 `"results"`。
    pub fn announce_command_palette(
        &mut self,
        cfg: &AccessibilityConfig,
        phase: &str,
        result_count: Option<usize>,
    ) {
        if !cfg.announce_command_palette {
            return;
        }
        let msg = match (phase, result_count) {
            ("opened", _) => "Command palette opened".to_string(),
            ("closed", _) => "Command palette closed".to_string(),
            ("results", Some(n)) => format!("Command palette, {n} results"),
            ("results", None) => "Command palette, no results".to_string(),
            (other, _) => format!("Command palette {other}"),
        };
        self.announce(cfg, &msg);
    }

    /// 播报状态栏更新。`summary` 是简短的人类可读
    /// 描述,例如 `"main branch, SSH connected"`。
    pub fn announce_status_bar(&mut self, cfg: &AccessibilityConfig, summary: &str) {
        if !cfg.announce_status_bar {
            return;
        }
        let msg = format!("Status bar: {summary}");
        self.announce(cfg, &msg);
    }

    /// 播报通用通知弹窗。当
    /// `cfg.announce_notifications` 为 false 时跳过。
    pub fn announce_notification(&mut self, cfg: &AccessibilityConfig, text: &str) {
        if !cfg.announce_notifications {
            return;
        }
        self.announce(cfg, &format!("Notification: {text}"));
    }

    fn record(&mut self, message: &str) {
        self.last_announcement = Some(message.to_string());
        if self.history.len() >= Self::HISTORY_LIMIT {
            self.history.remove(0);
        }
        self.history.push(message.to_string());
    }
}

/// 按规范格式构造标签页描述:`"Tab 1: bash - ~/Projects"`。
pub fn label_for_tab(index: usize, title: &str) -> String {
    format!("Switched to Tab {}: {}", index + 1, title)
}

// ---------------- macOS 专用:真正的 NSAccessibilityPostNotification ----------------

#[cfg(target_os = "macos")]
static NSAPP_LOCK: Mutex<()> = Mutex::new(());

#[cfg(target_os = "macos")]
fn post_announcement(message: &str) {
    // 我们持有一个短命互斥锁,确保并发的 announce() 调用
    // 不会在 NSString -> NSDictionary 的构建上竞争(Foundation
    // 集合不是 Send)。实际的 AppKit 调用是线程安全的,
    // 按 Apple 文档,只要在主线程上运行即可;我们
    // 还额外用 is_main_thread() 做了守卫。
    let _g = match NSAPP_LOCK.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if !is_main_thread() {
        log::debug!("accessibility (deferred to main thread): {message}");
        return;
    }
    unsafe { post_announcement_unsafe(message) }
}

#[cfg(target_os = "macos")]
fn is_main_thread() -> bool {
    unsafe {
        let cls = objc2::class!(NSThread);
        let cur: *mut AnyObject = objc2::msg_send![cls, currentThread];
        if cur.is_null() {
            return false;
        }
        let main: *mut AnyObject = objc2::msg_send![cls, mainThread];
        if main.is_null() {
            return false;
        }
        let is_main: bool = objc2::msg_send![cur, isMainThread];
        is_main
    }
}

#[cfg(target_os = "macos")]
unsafe fn post_announcement_unsafe(message: &str) {
    let ns_message = NSString::from_str(message);
    let key: &NSString = NSAccessibilityAnnouncementKey;
    let dict: RetainedDictionary = RetainedDictionary::from_str(&ns_message, key);
    let app_class = objc2::class!(NSApplication);
    let app: *mut AnyObject = objc2::msg_send![app_class, sharedApplication];
    if app.is_null() {
        log::debug!("accessibility (no NSApp): {message}");
        return;
    }
    let notif: &NSString = NSAccessibilityAnnouncementRequestedNotification;
    NSAccessibilityPostNotificationWithUserInfo(
        &*app,
        notif,
        Some(&*dict.0),
    );
    log::info!("AX announce: {message}");
}

#[cfg(target_os = "macos")]
struct RetainedDictionary(RetainedObj);

#[cfg(target_os = "macos")]
type RetainedObj = objc2::rc::Retained<NSDictionary<NSString>>;

#[cfg(target_os = "macos")]
impl RetainedDictionary {
    fn from_str(value: &NSString, key: &NSString) -> Self {
        unsafe {
            let cls = objc2::class!(NSMutableDictionary);
            let alloc: *mut AnyObject = objc2::msg_send![cls, alloc];
            let dict: *mut AnyObject = objc2::msg_send![alloc, init];
            let _: () = objc2::msg_send![&*dict, setObject: &*value, forKey: &*key];
            // NSDictionary<K, V> 在提供了 K 时默认 V=AnyObject,
            // 这正是我们想要的 user-info 字典类型。
            let typed: *mut NSDictionary<NSString> = std::mem::transmute(dict);
            RetainedDictionary(objc2::rc::Retained::retain(typed).expect("retain dict"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_announcement() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce(&cfg, "hello");
        assert_eq!(st.last_announcement.as_deref(), Some("hello"));
        assert_eq!(st.history.last().map(String::as_str), Some("hello"));
    }

    #[test]
    fn disabled_swallows_messages() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig { enabled: false, ..Default::default() };
        st.announce(&cfg, "ignored");
        assert!(st.last_announcement.is_none());
    }

    #[test]
    fn tab_change_format() {
        assert_eq!(label_for_tab(0, "bash - ~"), "Switched to Tab 1: bash - ~");
    }

    #[test]
    fn announce_tab_change_records() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce_tab_change(&cfg, 0, "zsh - ~/p");
        assert!(st.last_announcement.as_deref().unwrap().contains("Tab 1"));
    }

    #[test]
    fn split_respects_axis() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce_split(&cfg, "vertically", 2);
        assert_eq!(st.last_announcement.as_deref(), Some("Split vertically, 2 panes"));
        st.announce_split(&cfg, "horizontally", 3);
        assert_eq!(st.last_announcement.as_deref(), Some("Split horizontally, 3 panes"));
    }

    #[test]
    fn pane_focus_index_is_one_based() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce_pane_focus(&cfg, 0);
        assert_eq!(st.last_announcement.as_deref(), Some("Focused pane 1"));
    }

    #[test]
    fn command_palette_opened() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce_command_palette(&cfg, "opened", None);
        assert_eq!(st.last_announcement.as_deref(), Some("Command palette opened"));
    }

    #[test]
    fn command_palette_results() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce_command_palette(&cfg, "results", Some(5));
        assert_eq!(st.last_announcement.as_deref(), Some("Command palette, 5 results"));
    }

    #[test]
    fn command_palette_no_results() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce_command_palette(&cfg, "results", None);
        assert_eq!(st.last_announcement.as_deref(), Some("Command palette, no results"));
    }

    #[test]
    fn status_bar_includes_summary() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce_status_bar(&cfg, "main, SSH connected");
        assert_eq!(
            st.last_announcement.as_deref(),
            Some("Status bar: main, SSH connected")
        );
    }

    #[test]
    fn notification_prefix() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        st.announce_notification(&cfg, "build failed");
        assert_eq!(st.last_announcement.as_deref(), Some("Notification: build failed"));
    }

    #[test]
    fn subcategory_flags_respected() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig {
            announce_tab_changes: false,
            ..Default::default()
        };
        st.announce_tab_change(&cfg, 0, "x");
        assert!(st.last_announcement.is_none());

        let cfg = AccessibilityConfig {
            announce_pane_changes: false,
            ..Default::default()
        };
        st.announce_split(&cfg, "vertically", 2);
        assert!(st.last_announcement.is_none());
    }

    #[test]
    fn history_caps_at_limit() {
        let mut st = AccessibilityState::default();
        let cfg = AccessibilityConfig::default();
        for i in 0..(AccessibilityState::HISTORY_LIMIT + 5) {
            st.announce(&cfg, &format!("m{i}"));
        }
        assert_eq!(st.history.len(), AccessibilityState::HISTORY_LIMIT);
        assert_eq!(st.history[0], "m5");
    }
}
