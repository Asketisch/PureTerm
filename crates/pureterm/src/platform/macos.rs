//! macOS 平台特定窗口扩展
//!
//! 提供 Ghostty 风格的透明标题栏和玻璃效果支持。
//!
//! 使用 "Transparent Titlebar" 方案：
//! - `titlebarAppearsTransparent` 使标题栏透明
//! - `titleVisibility: hidden` 隐藏标题文字
//! - `FullSizeContentView` 让内容延伸到标题栏区域
//! - 红绿灯按钮保持在标题栏原位，内容视图在其下方渲染

use crate::platform::PlatformWindowExt;
use pureterm_core::config::GlassEffectStyle;

#[cfg(target_os = "macos")]
use objc2::class;

/// 获取 NSWindow 指针。
#[cfg(target_os = "macos")]
fn get_ns_window(window: &winit::window::Window) -> Option<*mut objc2::runtime::AnyObject> {
    use objc2::runtime::AnyObject;
    use raw_window_handle::HasWindowHandle;
    use raw_window_handle::RawWindowHandle;

    let handle = window.window_handle().ok()?;
    let appkit_handle = match handle.as_raw() {
        RawWindowHandle::AppKit(h) => h,
        _ => return None,
    };

    let ns_view_ptr = appkit_handle.ns_view.as_ptr();
    if ns_view_ptr.is_null() {
        return None;
    }

    unsafe {
        let ns_view: &AnyObject = &*(ns_view_ptr as *const AnyObject);
        let ns_window: *mut AnyObject = objc2::msg_send![ns_view, window];
        if ns_window.is_null() {
            None
        } else {
            Some(ns_window)
        }
    }
}

/// 应用透明标题栏样式（Ghostty 风格）。
#[cfg(target_os = "macos")]
fn apply_transparent_titlebar(ns_window: *mut objc2::runtime::AnyObject) {
    
    use objc2_foundation::{NSInteger, NSUInteger};

    unsafe {
        // titlebarAppearsTransparent = YES
        let _: () = objc2::msg_send![ns_window, setTitlebarAppearsTransparent: true];

        // titleVisibility = NSWindowTitleHidden (1)
        let vis: NSInteger = 1;
        let _: () = objc2::msg_send![ns_window, setTitleVisibility: vis];

        // styleMask |= NSFullSizeContentView (1 << 15)
        let current_style: NSUInteger = objc2::msg_send![ns_window, styleMask];
        let full_size_bit: NSUInteger = 1 << 15;
        let _: () = objc2::msg_send![ns_window, setStyleMask: current_style | full_size_bit];

        // 允许通过窗口背景拖拽
        let _: () = objc2::msg_send![ns_window, setMovableByWindowBackground: true];

        // 确保窗口有阴影
        let _: () = objc2::msg_send![ns_window, setHasShadow: true];

        log::info!(
            "macos: transparent titlebar applied (styleMask 0x{:x} -> 0x{:x})",
            current_style, current_style | full_size_bit
        );
    }
}

/// 确保红绿灯按钮可见并正确定位。
///
/// 不移动按钮位置，只确保它们不被隐藏。
/// macOS 的红绿灯按钮在 NSTitlebarContainerView 中，
/// 位于内容视图之上，应该自然可见。
#[cfg(target_os = "macos")]
fn ensure_traffic_lights_visible(ns_window: *mut objc2::runtime::AnyObject) {
    use objc2::runtime::AnyObject;


    unsafe {
        // NSWindowCloseButton=0, NSWindowMiniaturizeButton=1, NSWindowZoomButton=2
        for btn_type in [0u64, 1u64, 2u64] {
            let btn: *mut AnyObject = objc2::msg_send![ns_window, standardWindowButton: btn_type];
            if btn.is_null() {
                log::warn!("macos: standardWindowButton({btn_type}) is null");
                continue;
            }

            // 确保不被隐藏
            let _: () = objc2::msg_send![btn, setHidden: false];

            // 确保启用
            let _: () = objc2::msg_send![btn, setEnabled: true];

            log::info!("macos: button {btn_type} is visible, frame={:?}", {
                let frame: objc2_foundation::NSRect = objc2::msg_send![btn, frame];
                frame
            });
        }
    }
}

/// 创建红绿灯按钮区域的透明背景。
///
/// 当使用透明标题栏时，traffic lights 按钮后方需要有背景才能可见。
/// 使用纯色半透明背景，支持随窗口大小调整。
#[cfg(target_os = "macos")]
fn create_traffic_light_background(ns_window: *mut objc2::runtime::AnyObject) {
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSRect, NSUInteger, NSPoint, NSSize};

    unsafe {
        // 获取 contentView
        let content_view: *mut AnyObject = objc2::msg_send![ns_window, contentView];
        if content_view.is_null() {
            log::warn!("macos: contentView is null");
            return;
        }

        // 创建 NSView 作为背景
        let bg_view: *mut AnyObject = objc2::msg_send![class!(NSView), new];
        if bg_view.is_null() {
            log::warn!("macos: failed to create NSView");
            return;
        }

        // 设置 frame（覆盖红绿灯区域，大约 70x28）
        let frame = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize { width: 70.0, height: 28.0 },
        };
        let _: () = objc2::msg_send![bg_view, setFrame: frame];

        // 设置自动调整大小（向左扩展 + 固定高度）
        let autoresizing: NSUInteger = 2 | 16; // NSViewMaxXMargin | NSViewHeightSizable
        let _: () = objc2::msg_send![bg_view, setAutoresizingMask: autoresizing];

        // 设置透明背景
        let _: () = objc2::msg_send![bg_view, setWantsLayer: true];

        // 使用 CALayer 设置半透明背景色
        let layer: *mut AnyObject = objc2::msg_send![bg_view, layer];
        if !layer.is_null() {
            // 创建半透明灰色背景
            let bg_color: *mut AnyObject = objc2::msg_send![class!(NSColor), colorWithWhite:0.2 alpha:0.85];
            let _: () = objc2::msg_send![layer, setBackgroundColor: bg_color];
        }

        // 添加到 contentView（层级在按钮下方）
        let _: () = objc2::msg_send![content_view, addSubview: bg_view];

        log::info!("macos: created traffic light background view with transparent style");
    }
}

/// 应用平台特定窗口样式。
pub fn apply_platform_styling(
    window: &winit::window::Window,
    corner_radius: f32,
    glass_effect: &GlassEffectStyle,
) {
    #[cfg(target_os = "macos")]
    {
        if let Some(ns_window) = get_ns_window(window) {
            log::info!("macos: apply_platform_styling, NSWindow={:p}", ns_window);
            apply_transparent_titlebar(ns_window);
            ensure_traffic_lights_visible(ns_window);
            create_traffic_light_background(ns_window);
        } else {
            log::error!("macos: failed to get NSWindow");
        }
    }

    window.set_corner_radius(corner_radius);
    window.set_glass_effect(glass_effect.clone(), 20);
}

/// 窗口 resize 后无需额外操作（按钮位置由 macOS 管理）。
#[cfg(target_os = "macos")]
pub fn reposition_traffic_lights_on_resize(window: &winit::window::Window) {
    // 不需要重新定位，macOS 自动管理红绿灯按钮位置
    let _ = window;
}

impl PlatformWindowExt for winit::window::Window {
    fn set_corner_radius(&self, _radius: f32) {}
    fn set_glass_effect(&self, _style: GlassEffectStyle, _blur_radius: u32) {}
}
