//! GAP-09 / GAP-21:macOS Dock 徽标——未读通知 + Feed 待处理计数。
//!
//! 与 `macos_reopen.rs` / `macos_menu.rs` 类似:一个极小的 objc2-app-kit 垫片。
//! iced 的 `update()` 循环运行在主线程,因此可以直接设置徽标,
//! 无需派发跳转。`NSApplication::sharedApplication` 是 `MainThreadOnly`,
//! 与我们所持有的 `MainThreadMarker` 一致。

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;

/// 把 Dock 磁贴徽标设为给定计数,为 0 时清除;若不在主线程
/// 则空操作(防御性——调用点都在主线程)。
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

/// 清除 Dock 徽标(`set_badge(0)` 的便捷封装)。
pub fn clear_badge() {
    set_badge(0);
}
