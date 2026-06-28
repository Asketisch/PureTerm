//! GAP-09: macOS desktop notifications — UserNotifications framework toasts.
//!
//! Mirrors `macos_dock_badge.rs` / `macos_eyedropper.rs`: a small objc2 shim
//! whose every call site is the iced main thread. We request
//! `.alert | .sound | .badge` authorization once at startup (best-effort,
//! fire-and-forget — the completion handler is a no-op), then `post(title,
//! body)` schedules a `UNNotificationRequest` delivered immediately.
//!
//! macOS subtlety: the request is *dropped* if authorization was denied or has
//! not yet completed when `post` runs — that is acceptable (the in-app ring,
//! status-bar count, and Dock badge are unaffected; they don't go through
//! UNUserNotificationCenter). A proper bundle id is required for toasts to show
//! (see GAP-09 plan: repackage via `./build.sh app`).
//!
//! Version note: pinned to objc2-user-notifications **0.3.x** (objc2 0.6 line),
//! NOT the 0.2.x wry pulls transitively — 0.2.x binds to objc2 0.5 whose
//! `NSString`/`Retained` are distinct types from this crate's objc2 0.6.

use std::sync::atomic::{AtomicU64, Ordering};

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::ClassType;
use objc2_foundation::{NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound,
    UNUserNotificationCenter,
};

/// Per-process monotonic counter for notification identifiers. A stable/empty
/// identifier would make macOS coalesce or overwrite prior delivered
/// notifications; a fresh id per post keeps each toast distinct. (We don't need
/// stable ids for v1 — no click-to-focus-via-notification wiring yet.)
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Request notification authorization (`.alert | .sound | .badge`) once at
/// startup. Idempotent + best-effort: the completion handler does nothing, and a
/// toast posted before authorization resolves is simply dropped by the system.
/// A denied grant just means toasts won't show — call sites gate on
/// `[notification].desktop_notifications` and the focus/visibility logic, never
/// on the (async, unknown) authorization result.
pub fn request_authorization() {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let options = UNAuthorizationOptions::Alert
        | UNAuthorizationOptions::Sound
        | UNAuthorizationOptions::Badge;
    // Empty `Fn` block: authorization is fire-and-forget. block2 requires `Fn`
    // (not `FnOnce`); an empty closure is trivially `Fn`.
    let completion = RcBlock::new(move |_granted: Bool, _err: *mut NSError| {});
    center.requestAuthorizationWithOptions_completionHandler(options, &completion);
}

/// Post a desktop notification (title + body) immediately. The title is shown
/// as the alert headline, the body as the subtitle line. No-op side effect if
/// the app lacks/denied authorization (the system drops the request).
pub fn post(title: &str, body: &str) {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    // Fresh identifier per post so each toast is a distinct delivered
    // notification (empty/stable ids coalesce).
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let identifier = NSString::from_str(&format!("pureterm-{id}"));
    // `as_super()` upcasts the `&UNMutableNotificationContent` reference to the
    // `&UNNotificationContent` the constructor wants. `None` trigger ⇒ deliver
    // immediately.
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &identifier,
        content.as_super(),
        None,
    );
    // Completion handler is optional; we don't need the result (errors here mean
    // authorization denied — surfaced as "toast doesn't appear", already
    // expected).
    center.addNotificationRequest_withCompletionHandler(&request, None);
}
