//! GAP-09 / GAP-21: macOS Dock badge — notification unread + Feed pending count.
//!
//! Mirrors `macos_reopen.rs` / `macos_menu.rs`: a small objc2-app-kit shim. The
//! iced `update()` loop runs on the main thread, so the badge can be set directly
//! with no dispatch hop. `NSApplication::sharedApplication` is `MainThreadOnly`,
//! matching the `MainThreadMarker` we hold.

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;

/// Set the Dock tile badge to the given count, or clear it when 0. A no-op if not
/// on the main thread (defensive — the call sites are all on the main thread).
pub fn set_badge(count: u32) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let tile = app.dockTile();
    if count > 0 {
        let label = NSString::from_str(&count.to_string());
        tile.setBadgeLabel(Some(&label));
    } else {
        tile.setBadgeLabel(None);
    }
}

/// Clear the Dock badge (convenience for `set_badge(0)`).
pub fn clear_badge() {
    set_badge(0);
}
