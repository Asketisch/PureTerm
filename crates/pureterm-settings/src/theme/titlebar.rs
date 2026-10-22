//! GAP-72/73：标题栏样式 —— 让窗口顶部与界面主题**融为一体**。
//!
//! 三种模式:
//!
//! | 模式 | 平台 | 效果 |
//! |------|------|------|
//! | `native` | 全部 | 保留系统原生标题栏(Windows 上额外用 DWM 把标题栏**染成主题色**,消除割裂) |
//! | `overlay` | macOS | 窗口内容铺满标题栏区域,系统红绿灯浮在**应用自己的顶栏**上 |
//! | `unified` | 全部 | 隐藏系统标题栏,**应用自绘**标题栏(标题 + 最小化/最大化/关闭 + 拖动区) |
//!
//! macOS 默认 `overlay`(红绿灯融入顶栏);Windows / Linux 默认 `native`
//! (它们的标题栏归系统 / WM 所有,不能像 macOS 那样让系统按钮浮在内容上)。
//! Linux 用户想要同款观感时用 `unified`。
//!
//! 非 macOS 平台没有红绿灯可浮,`overlay` 会退化为 `native`。

use iced::window::settings::PlatformSpecific;

/// macOS 红绿灯的占位宽度(3 个 12px 按钮 + 间距 + 边距)。
pub const TRAFFIC_LIGHT_INSET: f32 = 78.0;

/// 标题栏高度(与顶栏同高,红绿灯垂直居中落在顶栏里)。
pub const TITLE_BAR_HEIGHT: f32 = 28.0;

/// 标题栏样式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TitleBarStyle {
    /// 系统原生标题栏(非 macOS 的默认)。
    #[default]
    Native,
    /// 内容铺满标题栏,系统按钮浮在界面顶栏上(macOS 默认)。
    Overlay,
    /// 应用自绘标题栏(隐藏系统标题栏 + 自绘窗口按钮)。
    Unified,
}

impl TitleBarStyle {
    /// 从配置字符串解析。未知值 → `Native`(坏配置不改变既有行为)。
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "overlay" => Self::Overlay,
            "unified" => Self::Unified,
            _ => Self::Native,
        }
    }

    /// 配置里写入的字符串形式。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Overlay => "overlay",
            Self::Unified => "unified",
        }
    }

    /// 设置窗口下拉框显示名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "Native(系统原生标题栏)",
            Self::Overlay => "Overlay(内容铺满标题栏,仅 macOS)",
            Self::Unified => "Unified(应用自绘标题栏)",
        }
    }

    /// 该平台是否真的支持这个模式。
    ///
    /// - `overlay` 只有 macOS 有意义(红绿灯浮在内容上);其它平台退化为 native;
    /// - `unified` 全平台可用。
    pub fn effective(self) -> Self {
        #[cfg(target_os = "macos")]
        {
            self
        }
        #[cfg(not(target_os = "macos"))]
        {
            match self {
                Self::Unified => Self::Unified,
                _ => Self::Native,
            }
        }
    }

    /// 是否需要给顶栏左侧留出红绿灯的位置。
    pub fn needs_traffic_light_inset(self) -> bool {
        self.effective() == Self::Overlay
    }

    /// 是否由应用自己绘制标题栏(隐藏系统标题栏)。
    pub fn draws_own_titlebar(self) -> bool {
        self.effective() == Self::Unified
    }

    /// 本平台的默认样式:macOS = overlay(红绿灯融入顶栏),其它 = native。
    pub fn platform_default() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::Overlay
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::Native
        }
    }

    /// 窗口的平台相关设置。
    ///
    /// macOS:`overlay` 时让内容铺满标题栏、标题栏透明、标题文字隐藏。
    /// 其它平台:保持默认(标题栏归系统 / WM 所有)。
    pub fn platform_specific(self) -> PlatformSpecific {
        #[cfg(target_os = "macos")]
        {
            let overlay = self.effective() == Self::Overlay;
            PlatformSpecific {
                title_hidden: overlay,
                titlebar_transparent: overlay,
                fullsize_content_view: overlay,
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            PlatformSpecific::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exact_match() {
        assert_eq!(TitleBarStyle::parse("native"), TitleBarStyle::Native);
        assert_eq!(TitleBarStyle::parse("overlay"), TitleBarStyle::Overlay);
        assert_eq!(TitleBarStyle::parse("unified"), TitleBarStyle::Unified);
        assert_eq!(TitleBarStyle::parse("  unified  "), TitleBarStyle::Unified);
    }

    #[test]
    fn parse_falls_back_to_native() {
        for bad in ["", "  ", "foo", "Overlay", "OVERLAY", "cyber", "隐藏"] {
            assert_eq!(
                TitleBarStyle::parse(bad),
                TitleBarStyle::Native,
                "非法标题栏样式 {bad:?} 必须回落 native"
            );
        }
    }

    #[test]
    fn roundtrip_and_options() {
        for style in [TitleBarStyle::Native, TitleBarStyle::Overlay, TitleBarStyle::Unified] {
            assert_eq!(TitleBarStyle::parse(style.as_str()), style);
            assert!(!style.label().is_empty());
        }
    }

    #[test]
    fn effective_is_platform_gated() {
        if cfg!(target_os = "macos") {
            assert_eq!(TitleBarStyle::Overlay.effective(), TitleBarStyle::Overlay);
            assert!(TitleBarStyle::Overlay.needs_traffic_light_inset());
        } else {
            // 非 macOS:overlay 退化为 native,但 unified 保留。
            assert_eq!(TitleBarStyle::Overlay.effective(), TitleBarStyle::Native);
            assert!(!TitleBarStyle::Overlay.needs_traffic_light_inset());
        }
        // unified 全平台可用。
        assert_eq!(TitleBarStyle::Unified.effective(), TitleBarStyle::Unified);
        assert!(TitleBarStyle::Unified.draws_own_titlebar());
        assert!(!TitleBarStyle::Native.draws_own_titlebar());
    }

    #[test]
    fn platform_specific_matches_style() {
        #[cfg(target_os = "macos")]
        {
            let overlay = TitleBarStyle::Overlay.platform_specific();
            assert!(overlay.title_hidden);
            assert!(overlay.titlebar_transparent);
            assert!(overlay.fullsize_content_view);

            let native = TitleBarStyle::Native.platform_specific();
            assert!(!native.title_hidden);
            assert!(!native.titlebar_transparent);
            assert!(!native.fullsize_content_view);
        }
    }
}
