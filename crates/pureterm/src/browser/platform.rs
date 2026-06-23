//! GAP-3.5: 取 iced 窗口的原生视图句柄(R1)。
//!
//! iced 0.14 的 daemon API 不向应用代码暴露 `&iced_winit::window::Window`,
//! 故无法直接拿到文档化的 `HasWindowHandle`。这里沿用
//! `iced_app::set_metal_presents_with_transaction` 已验证的 objc2 路径 ——
//! 走 `NSApplication.windows()` 拿到活动窗口的 `contentView`(NSView),
//! 包装成 `raw_window_handle` 的 `AppKitWindowHandle`,供 wry 的
//! `WebViewBuilder::build_as_child` 嵌入子 WKWebView。
//!
//! Phase A 仅 macOS;Linux(webkit2gtk/Wayland)与 Windows 嵌入留待后续。

#[cfg(target_os = "macos")]
use std::ffi::c_void;
#[cfg(target_os = "macos")]
use std::ptr::NonNull;

#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSWindow};

/// 一个实现 `HasWindowHandle` 的 NSView 包装。
///
/// 持有 iced content view 的裸指针:其生命周期由 NSApplication → NSWindow
/// 持有,只要该窗口未销毁即有效。调用方须保证在窗口关闭前不再使用此句柄
/// (关 pane/关窗时随 `BrowserContext` 一起 drop 掉 wry WebView)。
#[cfg(target_os = "macos")]
pub struct NsViewHandle {
    ns_view: NonNull<c_void>,
}

#[cfg(target_os = "macos")]
// SAFETY(for `borrow_raw` below): ns_view 是由 NSWindow 持有的 NSView,在窗口
// 存活期间地址稳定。BrowserContext 在关 pane/关窗前 drop 掉 wry WebView,不跨
// 窗口生命周期。
impl raw_window_handle::HasWindowHandle for NsViewHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let appkit = raw_window_handle::AppKitWindowHandle::new(self.ns_view);
        // SAFETY: 见 struct 注释 —— 指针在句柄使用期间有效。
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(raw_window_handle::RawWindowHandle::AppKit(
                appkit,
            ))
        })
    }
}

#[cfg(target_os = "macos")]
impl raw_window_handle::HasDisplayHandle for NsViewHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: AppKit RawDisplayHandle 无持有资源,borrow_raw 安全。
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(
                raw_window_handle::RawDisplayHandle::AppKit(
                    raw_window_handle::AppKitDisplayHandle::new(),
                ),
            )
        })
    }
}

/// 在主线程上取活动(key)窗口的 content NSView;若无 key 窗口,回退到
/// 第一个有 content view 的窗口。
///
/// 返回 `None` 时表示无法嵌入(非主线程 / 无窗口)—— 调用方应跳过本次
/// webview 实现(下一帧重试)。
///
/// 多窗口注意:当前取活动窗口,适合 MVP(主窗口内的 web pane)。
/// detached 窗口的精确匹配留待后续(给 NSWindow 打 identifier 标签)。
#[cfg(target_os = "macos")]
pub fn active_ns_view() -> Option<NsViewHandle> {
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);

    // 候选窗口:优先 key window(用户当前焦点),否则遍历所有窗口。
    let mut candidates: Vec<Retained<NSWindow>> = Vec::new();
    if let Some(key) = app.keyWindow() {
        candidates.push(key);
    }
    let all = app.windows();
    let count = all.count();
    for i in 0..count {
        let w = all.objectAtIndex(i);
        let ptr = Retained::as_ptr(&w);
        if !candidates.iter().any(|c| Retained::as_ptr(c) == ptr) {
            candidates.push(w);
        }
    }

    for w in candidates {
        if let Some(view) = w.contentView() {
            let ptr = Retained::as_ptr(&view) as *mut c_void;
            let ns_view = NonNull::new(ptr)?;
            return Some(NsViewHandle { ns_view });
        }
    }
    None
}
