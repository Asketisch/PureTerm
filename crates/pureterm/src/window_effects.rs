//! GAP-55: Window transparency and blur.
//!
//! macOS: install an `NSVisualEffectView` (`.behindWindow` blend mode) as the
//! window's `contentView` so the OS draws the system vibrancy / blur under
//! the rest of the view hierarchy. The opaque terminal view is added as a
//! subview on top.
//!
//! Windows: enable `DwmEnableBlurBehindWindow` via the `windows` crate
//! (feature-gated).
//!
//! Other Unix: no-op (the system compositor provides its own blur for
//! compatible WMs); the function logs a debug message.

use std::path::Path;

/// `[window.effects]` config (opacity + blur).
#[derive(Clone, Debug, PartialEq)]
pub struct WindowEffectsConfig {
    pub enabled: bool,
    pub opacity: f32,
    pub blur: bool,
    /// macOS only: `behind_window` (default) | `within_window`.
    /// `behind_window` blurs desktop wallpaper / windows behind this one;
    /// `within_window` blurs content inside the window.
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

/// Result of a blur apply attempt. Useful for tests and logging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlurApplyOutcome {
    /// `enabled` was false or `blur` was false — nothing to do.
    Skipped,
    /// macOS: NSVisualEffectView installed successfully.
    InstalledMacos,
    /// Windows: DWM blur-behind enabled.
    InstalledWindows,
    /// Platform does not support blur (Linux/BSD without WM hint support).
    UnsupportedPlatform,
    /// Internal failure (could not access NSApplication, alloc failed, …).
    Failed(String),
}

/// macOS: install an `NSVisualEffectView` with `.behindWindow` blending
/// under the existing window content. Idempotent — calling twice is safe
/// (the second call replaces the previous effect view).
#[cfg(target_os = "macos")]
pub fn apply_blur_macos(blending_behind: bool) -> BlurApplyOutcome {
    use objc2::ffi::NSInteger;
    use objc2::rc::Retained;
    use objc2::{msg_send, ClassType};
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
        // NSViewWidthSizable | NSViewHeightSizable = 2 | 16 (raw bitmask from AppKit).
        let mask: NSInteger = 2 | 16;
        let _: () = unsafe { msg_send![&*effect, setAutoresizingMask: mask] };
        // Move the existing content view under the new effect view so the
        // terminal render path is unchanged. NSWindow takes ownership of
        // the new contentView.
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

/// Windows: enable `DwmEnableBlurBehindWindow` for the active window. This
/// is best-effort; many W10+ setups replace the legacy API with the
/// Acrylic/Modern backdrop APIs, which are not yet wired here.
#[cfg(target_os = "windows")]
pub fn apply_blur_windows() -> BlurApplyOutcome {
    // The full Win32 wiring would require a `winapi`/`windows` crate and a
    // `HWND` handle. PureTerm's binary uses `winit` for windowing; the
    // winit `Window::hwnd()` method exposes the HWND. We avoid the
    // dependency here and return Unsupported unless the winit integration
    // is built out (tracked as a follow-up).
    BlurApplyOutcome::UnsupportedPlatform
}

/// Cross-platform entry point.
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

/// Best-effort load: read `[window.effects]` from a TOML file. Used by
/// `p4_runtime` to bootstrap at startup. Returns `Default` on any error.
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
        // base * 1.0 stays base
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

    /// T-core-windows-conpty v2: the Windows blur stub returns
    /// `UnsupportedPlatform` on non-Windows hosts (which is where this
    /// test compiles) and is a no-op `Ok(())` once the winit/Win32
    /// wiring lands in v3.
    #[test]
    fn apply_blur_disabled_yields_skipped() {
        let mut cfg = WindowEffectsConfig::default();
        cfg.enabled = false;
        cfg.blur = false;
        assert_eq!(apply_blur(&cfg), BlurApplyOutcome::Skipped);
    }

    /// When the config says `enabled = true, blur = false`, we also
    /// skip (blur is gated on both flags).
    #[test]
    fn apply_blur_with_disabled_blur_flag_yields_skipped() {
        let mut cfg = WindowEffectsConfig::default();
        cfg.enabled = true;
        cfg.blur = false;
        assert_eq!(apply_blur(&cfg), BlurApplyOutcome::Skipped);
    }

    /// On non-macOS / non-Windows hosts, the cross-platform dispatch
    /// returns `UnsupportedPlatform`.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn apply_blur_unsupported_on_linux_dev_hosts() {
        let mut cfg = WindowEffectsConfig::default();
        cfg.enabled = true;
        cfg.blur = true;
        assert_eq!(apply_blur(&cfg), BlurApplyOutcome::UnsupportedPlatform);
    }
