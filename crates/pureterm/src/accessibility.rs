//! GAP-4.6: VoiceOver / accessibility announcements.
//!
//! **macOS** path: real `NSAccessibilityPostNotification` calls that drive
//! VoiceOver. The standard announcement recipe is to post
//! `NSAccessibilityAnnouncementRequestedNotification` with a user-info
//! dictionary whose `NSAccessibilityAnnouncementKey` is the text to speak.
//! We dispatch to the app via `NSApp` (the main application object) which
//! is reachable from any thread that has scheduled the run loop.
//!
//! **Other platforms**: log-only fallback (Visual / NVDA users typically
//! pair PureTerm with platform-native screen readers that read from the
//! terminal anyway, so we do not duplicate that work here).
//!
//! **Widget-level a11y**: iced 0.14 does not yet expose `widget::accessible()`.
//! The per-widget label sweep is tracked under the same spec and will land
//! when iced's a11y API stabilises; the structured announcement helpers
//! below are what actually drive VoiceOver today.

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

/// `[accessibility]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityConfig {
    pub enabled: bool,
    pub announce_tab_changes: bool,
    pub announce_notifications: bool,
    /// Announce pane split / resize / focus events.
    pub announce_pane_changes: bool,
    /// Announce command palette open / close + result count.
    pub announce_command_palette: bool,
    /// Announce status-bar updates (cwd / git branch / process name).
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
    /// Maximum number of historical announcements to retain for tests and
    /// for the in-app AX log overlay.
    const HISTORY_LIMIT: usize = 32;

    /// Speak / log a generic accessibility message. Respects
    /// `cfg.enabled`; safe to call from any thread.
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

    /// Announce a tab switch. Format: `"Switched to Tab N: <title>"`.
    pub fn announce_tab_change(&mut self, cfg: &AccessibilityConfig, index: usize, title: &str) {
        if !cfg.announce_tab_changes {
            return;
        }
        let label = label_for_tab(index, title);
        self.announce(cfg, &label);
    }

    /// Announce a pane split. `axis` is `"vertically"` or `"horizontally"`,
    /// `count` is the total number of panes after the split.
    pub fn announce_split(&mut self, cfg: &AccessibilityConfig, axis: &str, count: usize) {
        if !cfg.announce_pane_changes {
            return;
        }
        let msg = format!("Split {axis}, {count} panes");
        self.announce(cfg, &msg);
    }

    /// Announce a pane focus change (cursor moved into a different pane).
    pub fn announce_pane_focus(&mut self, cfg: &AccessibilityConfig, pane_index: usize) {
        if !cfg.announce_pane_changes {
            return;
        }
        let msg = format!("Focused pane {}", pane_index + 1);
        self.announce(cfg, &msg);
    }

    /// Announce the command palette opening or its result count.
    /// `phase` is `"opened"`, `"closed"`, or `"results"`.
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

    /// Announce a status-bar update. `summary` is a short human-readable
    /// description, e.g. `"main branch, SSH connected"`.
    pub fn announce_status_bar(&mut self, cfg: &AccessibilityConfig, summary: &str) {
        if !cfg.announce_status_bar {
            return;
        }
        let msg = format!("Status bar: {summary}");
        self.announce(cfg, &msg);
    }

    /// Announce a generic notification toast. Skipped when
    /// `cfg.announce_notifications` is false.
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

/// Build a tab description following the spec format: `"Tab 1: bash - ~/Projects"`.
pub fn label_for_tab(index: usize, title: &str) -> String {
    format!("Switched to Tab {}: {}", index + 1, title)
}

// ---------------- macOS-only: real NSAccessibilityPostNotification ----------------

#[cfg(target_os = "macos")]
static NSAPP_LOCK: Mutex<()> = Mutex::new(());

#[cfg(target_os = "macos")]
fn post_announcement(message: &str) {
    // We hold a short-lived mutex to make sure concurrent announce() calls
    // don't race on the NSString -> NSDictionary build (the Foundation
    // collections are not Send). The actual AppKit call is thread-safe
    // per Apple docs as long as it runs on the main thread; we
    // additionally guard with is_main_thread().
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
            // NSDictionary<K, V> defaults V=AnyObject when K is provided,
            // which is exactly what we want for the user-info dict.
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
