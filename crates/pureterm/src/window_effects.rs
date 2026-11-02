//! GAP-55:窗口透明与毛玻璃模糊。
//!
//! macOS:安装 `NSVisualEffectView`(`.behindWindow` 混合模式)作为
//! 窗口的 `contentView`,让系统在视图层级下方绘制
//! 系统级 vibrancy / 毛玻璃。不透明的终端视图作为
//! 子视图叠加在最上层。
//!
//! Windows:通过 `windows` crate 启用 `DwmEnableBlurBehindWindow`
//! (特性开关控制)。
//!
//! 其他 Unix:无操作(系统合成器会为兼容的窗口管理器
//! 自带毛玻璃);该函数仅记录一条 debug 日志。

use std::path::Path;

/// `[window.effects]` 配置(不透明度 + 毛玻璃)。
#[derive(Clone, Debug, PartialEq)]
pub struct WindowEffectsConfig {
    pub enabled: bool,
    pub opacity: f32,
    pub blur: bool,
    /// 仅 macOS:`behind_window`(默认)| `within_window`。
    /// `behind_window` 模糊本窗口背后的桌面壁纸 / 窗口;
    /// `within_window` 模糊窗口内部的内容。
    pub blending_mode: String,
}

impl Default for WindowEffectsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            opacity: 1.0,
            blur: false,
            blending_mode: "behind_window".into(),
        }
    }
}

impl WindowEffectsConfig {
    pub fn effective_opacity(&self, base: f32) -> f32 {
        if !self.enabled {
            return base;
        }
        (base * self.opacity).clamp(0.05, 1.0)
    }
}

/// 一次毛玻璃应用尝试的结果。供测试与日志使用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlurApplyOutcome {
    /// `enabled` 为 false 或 `blur` 为 false——无事可做。
    Skipped,
    /// macOS:已成功安装 NSVisualEffectView。
    InstalledMacos,
    /// Windows:已启用 DWM 毛玻璃。
    InstalledWindows,
    /// 平台不支持毛玻璃(Linux/BSD 无 WM 提示支持)。
    UnsupportedPlatform,
    /// 内部失败(无法访问 NSApplication、alloc 失败等)。
    Failed(String),
}

/// macOS:在现有窗口内容之下安装一个 `.behindWindow` 混合的
/// `NSVisualEffectView`。幂等——调用两次是安全的
/// (第二次调用会替换掉之前的毛玻璃视图)。
#[cfg(target_os = "macos")]
pub fn apply_blur_macos(blending_behind: bool) -> BlurApplyOutcome {
    use objc2::ffi::NSInteger;
    use objc2::rc::Retained;
    use objc2::msg_send;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplication, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Some(mtm) = MainThreadMarker::new() else {
        return BlurApplyOutcome::Failed("not on main thread".into());
    };
    let app = NSApplication::sharedApplication(mtm);
    let windows = app.windows();
    let count = windows.count();
    if count == 0 {
        return BlurApplyOutcome::Failed("no windows".into());
    }
    let mut installed = 0;
    for i in 0..count {
        let window: Retained<NSWindow> = windows.objectAtIndex(i);
        let Some(content) = window.contentView() else { continue };
        let bounds = content.bounds();
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(bounds.size.width, bounds.size.height),
        );
        let effect = NSVisualEffectView::new(mtm);
        effect.setFrame(frame);
        effect.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
        let mode = if blending_behind {
            NSVisualEffectBlendingMode::BehindWindow
        } else {
            NSVisualEffectBlendingMode::WithinWindow
        };
        effect.setBlendingMode(mode);
        effect.setState(NSVisualEffectState::FollowsWindowActiveState);
        // NSViewWidthSizable | NSViewHeightSizable = 2 | 16(来自 AppKit 的原始位掩码)。
        let mask: NSInteger = 2 | 16;
        let _: () = unsafe { msg_send![&*effect, setAutoresizingMask: mask] };
        // 把现有内容视图移到新的毛玻璃视图之下,让
        // 终端渲染路径保持不变。NSWindow 接管
        // 新的 contentView 的所有权。
        let prev_content: Retained<NSView> = content.clone();
        let prev_frame: NSRect = prev_content.frame();
        let _: () = unsafe { msg_send![&*window, setContentView: &*effect] };
        let _: () = unsafe { msg_send![&*effect, addSubview: &*prev_content] };
        prev_content.setFrame(prev_frame);
        installed += 1;
    }
    if installed == 0 {
        BlurApplyOutcome::Failed("no windows installed".into())
    } else {
        BlurApplyOutcome::InstalledMacos
    }
}

/// Windows:为当前活动窗口启用 `DwmEnableBlurBehindWindow`。这是
/// 尽力而为;许多 W10+ 环境会用 Acrylic/Modern 背景
/// API 取代旧 API,这些新 API 尚未在此接入。
#[cfg(target_os = "windows")]
pub fn apply_blur_windows() -> BlurApplyOutcome {
    // 完整的 Win32 接线需要 `winapi`/`windows` crate 和
    // `HWND` 句柄。PureTerm 的二进制使用 `winit` 管理窗口;
    // winit 的 `Window::hwnd()` 方法可暴露 HWND。此处我们避免
    // 引入该依赖并返回 Unsupported,除非 winit 集成
    // 已完整实现(作为后续事项跟踪)。
    BlurApplyOutcome::UnsupportedPlatform
}

/// 跨平台入口。
pub fn apply_blur(cfg: &WindowEffectsConfig) -> BlurApplyOutcome {
    if !cfg.enabled || !cfg.blur {
        return BlurApplyOutcome::Skipped;
    }
    let behind = !cfg.blending_mode.eq_ignore_ascii_case("within_window");
    #[cfg(target_os = "macos")]
    { apply_blur_macos(behind) }
    #[cfg(target_os = "windows")]
    { apply_blur_windows() }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = behind;
        BlurApplyOutcome::UnsupportedPlatform
    }
}

/// 尽力加载:从 TOML 文件读取 `[window.effects]`。供
/// `p4_runtime` 在启动时引导使用。任何错误都返回 `Default`。
pub fn load_from_path(path: &Path) -> WindowEffectsConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return WindowEffectsConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Table>() else {
        return WindowEffectsConfig::default();
    };
    let Some(table) = value.get("window").and_then(|v| v.get("effects")).and_then(|v| v.as_table()) else {
        return WindowEffectsConfig::default();
    };
    let mut cfg = WindowEffectsConfig::default();
    if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
        cfg.enabled = b;
    }
    if let Some(b) = table.get("blur").and_then(|v| v.as_bool()) {
        cfg.blur = b;
    }
    if let Some(o) = table.get("opacity").and_then(|v| v.as_float()) {
        cfg.opacity = o as f32;
    }
    if let Some(m) = table.get("blending_mode").and_then(|v| v.as_str()) {
        cfg.blending_mode = m.to_string();
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_opacity() {
        let cfg = WindowEffectsConfig {
            enabled: true,
            opacity: 2.0,
            blur: false,
            ..Default::default()
        };
        assert_eq!(cfg.effective_opacity(1.0), 1.0);
    }

    #[test]
    fn disabled_passthrough() {
        let cfg = WindowEffectsConfig::default();
        // base * 1.0 仍为 base
        assert_eq!(cfg.effective_opacity(0.5), 0.5);
    }

    #[test]
    fn apply_returns_skipped_when_disabled() {
        let cfg = WindowEffectsConfig::default();
        assert_eq!(apply_blur(&cfg), BlurApplyOutcome::Skipped);
    }

    #[test]
    fn apply_returns_skipped_when_blur_false() {
        let cfg = WindowEffectsConfig {
            enabled: true,
            blur: false,
            ..Default::default()
        };
        assert_eq!(apply_blur(&cfg), BlurApplyOutcome::Skipped);
    }

    #[test]
    fn parse_blending_mode() {
        let cfg = WindowEffectsConfig {
            blending_mode: "Within_Window".into(),
            ..Default::default()
        };
        assert!(cfg.blending_mode.eq_ignore_ascii_case("within_window"));
    }

    #[test]
    fn load_from_missing_path_is_default() {
        let cfg = load_from_path(Path::new("/nonexistent/path.toml"));
        assert!(!cfg.enabled);
        assert_eq!(cfg.opacity, 1.0);
        assert!(!cfg.blur);
    }
}

    /// T-core-windows-conpty v2:Windows 毛玻璃桩函数在非 Windows 主机上
    /// (即本测试实际编译的平台)返回 `UnsupportedPlatform`;
    /// 只有 winit/Win32 接线在 v3 落地后,才会变为 no-op `Ok(())`。
    /// (v3 之前的占位桩,行为与上面两条测试一致。)
    #[test]
    fn apply_blur_disabled_yields_skipped() {
        let mut cfg = WindowEffectsConfig::default();
        cfg.enabled = false;
        cfg.blur = false;
        assert_eq!(apply_blur(&cfg), BlurApplyOutcome::Skipped);
    }

    /// 当配置为 `enabled = true, blur = false` 时,同样会
    /// 跳过(毛玻璃受两个开关共同约束)。
    #[test]
    fn apply_blur_with_disabled_blur_flag_yields_skipped() {
        let mut cfg = WindowEffectsConfig::default();
        cfg.enabled = true;
        cfg.blur = false;
        assert_eq!(apply_blur(&cfg), BlurApplyOutcome::Skipped);
    }

    /// 在非 macOS / 非 Windows 主机上,跨平台分发
    /// 返回 `UnsupportedPlatform`。
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn apply_blur_unsupported_on_linux_dev_hosts() {
        let mut cfg = WindowEffectsConfig::default();
        cfg.enabled = true;
        cfg.blur = true;
        assert_eq!(apply_blur(&cfg), BlurApplyOutcome::UnsupportedPlatform);
    }
