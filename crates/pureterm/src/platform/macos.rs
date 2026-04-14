//! macOS 平台特定窗口扩展
//!
//! 提供圆角和玻璃效果支持 via AppKit APIs。

use crate::platform::PlatformWindowExt;

/// 应用平台特定窗口样式
pub fn apply_platform_styling(window: &winit::window::Window, corner_radius: f32, glass_effect: &pureterm_core::config::GlassEffectStyle) {
    window.set_corner_radius(corner_radius);
    window.set_glass_effect(glass_effect.clone(), 20);
}

impl PlatformWindowExt for winit::window::Window {
    /// 设置窗口圆角半径
    ///
    /// 注意：圆角裁剪主要由 shader（terminal.wgsl）实现。
    /// macOS 上标准窗口装饰已包含圆角效果。
    /// 对于完全自定义的圆角窗口，需要使用 winit 未暴露的 NSWindow.setCornerRadius。
    /// Ghostty 使用未文档化 API：window._cornerRadius
    fn set_corner_radius(&self, _radius: f32) {
        #[cfg(target_os = "macos")]
        {
            // 圆角由 shader 处理，macOS 窗口本身使用系统装饰
        }
    }

    /// 设置窗口玻璃效果
    ///
    /// 玻璃效果（NSVisualEffectView）需要：
    /// 1. 从 raw-window-handle 获取 NSWindow
    /// 2. 创建 NSVisualEffectView
    /// 3. 作为窗口内容视图添加
    ///
    /// 这需要 objc2 crate 或 Swift helper 配合实现，较为复杂。
    /// 目前透明度通过 wgpu 的 alpha_mode 启用。
    /// Ghostty 通过 Swift helper ghostty_set_window_background_blur() 实现此功能。
    fn set_glass_effect(&self, style: pureterm_core::config::GlassEffectStyle, _blur_radius: u32) {
        #[cfg(target_os = "macos")]
        {
            // 玻璃效果需要 AppKit 的 NSVisualEffectView
            // 当前透明度通过 wgpu alpha_mode 处理
        }
    }
}
