//! 平台特定窗口扩展
//!
//! 提供平台相关功能特性，如圆角和玻璃效果。

use pureterm_core::config::GlassEffectStyle;

/// 平台特定窗口扩展 trait
pub trait PlatformWindowExt {
    /// 设置窗口圆角半径
    fn set_corner_radius(&self, radius: f32);

    /// 设置窗口玻璃效果（macOS）
    fn set_glass_effect(&self, style: GlassEffectStyle, blur_radius: u32);
}

#[cfg(target_os = "macos")]
pub mod macos;
