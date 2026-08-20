//! `load_*` 方法 —— 其余 crate 子模块配置的加载器。
//!
//! 本文件是 `impl AppConfig` 的拆分块,包含各类杂项子配置(glass 效果、触控栏、
//! 视觉 tab 栏、窗口边框、徽标叠加、文件预览、文件传输、共享会话、自动更新、
//! 插件系统、云同步、workflows、notebooks、背景图、窗口特效、鼠标焦点跟随、
//! 触感反馈、自定义主题、emoji 渲染、面板捕获、打印支持、无障碍)的静态加载方法。
//! 这些方法体均直接使用 `crate::<module>::Type` 的全限定路径,因此本文件
//! 仅需导入 `Path`/`PathBuf`。

use std::path::{Path, PathBuf};

use super::AppConfig;

impl AppConfig {
    /// GAP-20: `[window.glass]` —— 毛玻璃特效
    pub fn load_glass_effect(path: &Path) -> crate::macos_glass_effect::GlassEffectConfig {
        let mut cfg = crate::macos_glass_effect::GlassEffectConfig::default();
        if let Some(table) = Self::load_table(path, "window") {
            if let Some(glass) = table.get("glass").and_then(|v| v.as_table()) {
                if let Some(b) = glass.get("enabled").and_then(|v| v.as_bool()) {
                    cfg.enabled = b;
                }
                if let Some(s) = glass.get("style").and_then(|v| v.as_str()) {
                    cfg.style = s.to_string();
                }
            }
        }
        cfg
    }

    /// GAP-22: `[touch_bar]` —— 触控栏
    pub fn load_touch_bar(path: &Path) -> crate::macos_touch_bar::TouchBarConfig {
        let mut cfg = crate::macos_touch_bar::TouchBarConfig::default();
        let Some(table) = Self::load_table(path, "touch_bar") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("show_tabs").and_then(|v| v.as_bool()) {
            cfg.show_tabs = b;
        }
        cfg
    }

    /// GAP-23: `[tab_bar]` —— 视觉标签栏
    pub fn load_visual_tab_bar(path: &Path) -> crate::visual_tab_bar::VisualTabBarConfig {
        let mut cfg = crate::visual_tab_bar::VisualTabBarConfig::default();
        let Some(table) = Self::load_table(path, "tab_bar") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("position").and_then(|v| v.as_str()) {
            cfg.position = crate::visual_tab_bar::TabBarPosition::parse(s);
        }
        if let Some(n) = table.get("height").and_then(|v| v.as_float()) {
            cfg.height = n as f32;
        }
        cfg
    }

    /// GAP-28: `[window.border]` —— 窗口边框
    pub fn load_window_border(path: &Path) -> crate::window_border::WindowBorderConfig {
        let mut cfg = crate::window_border::WindowBorderConfig::default();
        if let Some(table) = Self::load_table(path, "window") {
            if let Some(border) = table.get("border").and_then(|v| v.as_table()) {
                if let Some(b) = border.get("enabled").and_then(|v| v.as_bool()) {
                    cfg.enabled = b;
                }
                if let Some(n) = border.get("width").and_then(|v| v.as_float()) {
                    cfg.width = n as f32;
                }
                if let Some(s) = border.get("active_color").and_then(|v| v.as_str()) {
                    cfg.active_color = s.to_string();
                }
                if let Some(s) = border.get("inactive_color").and_then(|v| v.as_str()) {
                    cfg.inactive_color = s.to_string();
                }
            }
        }
        cfg
    }

    /// GAP-29: `[badge]` —— 徽标叠加
    pub fn load_badge_overlay(path: &Path) -> crate::badge_overlay::BadgeOverlayConfig {
        let mut cfg = crate::badge_overlay::BadgeOverlayConfig::default();
        let Some(table) = Self::load_table(path, "badge") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("text").and_then(|v| v.as_str()) {
            cfg.text = s.to_string();
        }
        if let Some(s) = table.get("color").and_then(|v| v.as_str()) {
            cfg.color = s.to_string();
        }
        if let Some(s) = table.get("position").and_then(|v| v.as_str()) {
            cfg.position = crate::badge_overlay::BadgePosition::parse(s);
        }
        if let Some(n) = table.get("opacity").and_then(|v| v.as_float()) {
            cfg.opacity = n as f32;
        }
        cfg
    }

    /// GAP-34: `[file_preview]` —— 文件预览
    pub fn load_file_preview(path: &Path) -> crate::file_preview::FilePreviewConfig {
        let mut cfg = crate::file_preview::FilePreviewConfig::default();
        let Some(table) = Self::load_table(path, "file_preview") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        cfg
    }

    /// GAP-38: `[file_transfer]` —— 文件传输
    pub fn load_file_transfer(path: &Path) -> crate::file_transfer::FileTransferConfig {
        let mut cfg = crate::file_transfer::FileTransferConfig::default();
        let Some(table) = Self::load_table(path, "file_transfer") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("prefer_sftp").and_then(|v| v.as_bool()) {
            cfg.prefer_sftp = b;
        }
        cfg
    }

    /// GAP-44: `[shared_session]` —— 共享会话
    pub fn load_shared_session(path: &Path) -> crate::shared_sessions::SharedSessionConfig {
        let mut cfg = crate::shared_sessions::SharedSessionConfig::default();
        let Some(table) = Self::load_table(path, "shared_session") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(n) = table.get("port").and_then(|v| v.as_integer()) {
            cfg.port = (n.max(1).min(65535)) as u16;
        }
        if let Some(b) = table.get("read_only_default").and_then(|v| v.as_bool()) {
            cfg.read_only_default = b;
        }
        cfg
    }

    /// GAP-45: `[update]` —— 自动更新
    pub fn load_auto_update(path: &Path) -> crate::auto_update::AutoUpdateConfig {
        let mut cfg = crate::auto_update::AutoUpdateConfig::default();
        let Some(table) = Self::load_table(path, "update") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("auto_check").and_then(|v| v.as_bool()) {
            cfg.auto_check = b;
        }
        if let Some(s) = table.get("channel").and_then(|v| v.as_str()) {
            cfg.channel = s.to_string();
        }
        if let Some(s) = table.get("feed_url").and_then(|v| v.as_str()) {
            cfg.feed_url = s.to_string();
        }
        cfg
    }

    /// GAP-49: `[plugins]` —— 插件系统
    pub fn load_plugins(path: &Path) -> crate::plugin_system::PluginSystemConfig {
        let mut cfg = crate::plugin_system::PluginSystemConfig::default();
        let Some(table) = Self::load_table(path, "plugins") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("directory").and_then(|v| v.as_str()) {
            cfg.directory = Some(PathBuf::from(s));
        }
        cfg
    }

    /// GAP-50: `[cloud_sync]` —— 云同步
    pub fn load_cloud_sync(path: &Path) -> crate::cloud_sync::CloudSyncConfig {
        let mut cfg = crate::cloud_sync::CloudSyncConfig::default();
        let Some(table) = Self::load_table(path, "cloud_sync") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("gist_id").and_then(|v| v.as_str()) {
            cfg.gist_id = s.to_string();
        }
        if let Some(s) = table.get("token_env").and_then(|v| v.as_str()) {
            cfg.token_env = s.to_string();
        }
        cfg
    }

    /// GAP-52: `[workflows]` 启用开关(条目经 `[[workflows]]` 定义)。
    pub fn load_workflows_enabled(path: &Path) -> crate::workflows::WorkflowsConfig {
        let mut cfg = crate::workflows::WorkflowsConfig::default();
        let Some(table) = Self::load_table(path, "workflows") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        cfg
    }

    /// GAP-53: `[notebooks]` —— 记事本
    pub fn load_notebooks(path: &Path) -> crate::notebooks::NotebooksConfig {
        let mut cfg = crate::notebooks::NotebooksConfig::default();
        let Some(table) = Self::load_table(path, "notebooks") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        cfg
    }

    /// GAP-54: `[background]` —— 背景图
    pub fn load_background_image(path: &Path) -> crate::background_image::BackgroundImageConfig {
        let mut cfg = crate::background_image::BackgroundImageConfig::default();
        let Some(table) = Self::load_table(path, "background") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("image").and_then(|v| v.as_str()) {
            cfg.image = PathBuf::from(s);
        }
        if let Some(n) = table.get("opacity").and_then(|v| v.as_float()) {
            cfg.opacity = n as f32;
        }
        if let Some(s) = table.get("mode").and_then(|v| v.as_str()) {
            cfg.mode = crate::background_image::BackgroundMode::parse(s);
        }
        cfg
    }

    /// GAP-55: `[window.effects]` —— 窗口特效
    pub fn load_window_effects(path: &Path) -> crate::window_effects::WindowEffectsConfig {
        let mut cfg = crate::window_effects::WindowEffectsConfig::default();
        if let Some(table) = Self::load_table(path, "window") {
            if let Some(fx) = table.get("effects").and_then(|v| v.as_table()) {
                if let Some(b) = fx.get("enabled").and_then(|v| v.as_bool()) {
                    cfg.enabled = b;
                }
                if let Some(n) = fx.get("opacity").and_then(|v| v.as_float()) {
                    cfg.opacity = n as f32;
                }
                if let Some(b) = fx.get("blur").and_then(|v| v.as_bool()) {
                    cfg.blur = b;
                }
            }
        }
        cfg
    }

    /// GAP-56: `[focus_follows_mouse]` —— 鼠标焦点跟随
    pub fn load_focus_follows_mouse(path: &Path) -> crate::focus_follows_mouse::FocusFollowsMouseConfig {
        let mut cfg = crate::focus_follows_mouse::FocusFollowsMouseConfig::default();
        let Some(table) = Self::load_table(path, "focus_follows_mouse") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(n) = table.get("delay_ms").and_then(|v| v.as_integer()) {
            cfg.delay_ms = (n.max(0)) as u64;
        }
        cfg
    }

    /// GAP-57: `[haptic]` —— 触感反馈
    pub fn load_haptic(path: &Path) -> crate::macos_haptic::HapticConfig {
        let mut cfg = crate::macos_haptic::HapticConfig::default();
        let Some(table) = Self::load_table(path, "haptic") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("on_tab_switch").and_then(|v| v.as_bool()) {
            cfg.on_tab_switch = b;
        }
        cfg
    }

    /// GAP-58: `[theme]` —— 自定义主题
    pub fn load_custom_theme(path: &Path) -> crate::custom_theme::CustomThemeConfig {
        let mut cfg = crate::custom_theme::CustomThemeConfig::default();
        let Some(table) = Self::load_table(path, "theme") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("sidebar_bg").and_then(|v| v.as_str()) {
            cfg.sidebar_bg = s.to_string();
        }
        if let Some(s) = table.get("tab_bar_bg").and_then(|v| v.as_str()) {
            cfg.tab_bar_bg = s.to_string();
        }
        if let Some(s) = table.get("status_bar_bg").and_then(|v| v.as_str()) {
            cfg.status_bar_bg = s.to_string();
        }
        if let Some(b) = table.get("follow_system").and_then(|v| v.as_bool()) {
            cfg.follow_system = b;
        }
        cfg
    }

    /// GAP-59: `[emoji]` —— emoji 渲染
    pub fn load_emoji_rendering(path: &Path) -> crate::emoji_rendering::EmojiRenderingConfig {
        let mut cfg = crate::emoji_rendering::EmojiRenderingConfig::default();
        let Some(table) = Self::load_table(path, "emoji") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("dedicated_atlas").and_then(|v| v.as_bool()) {
            cfg.dedicated_atlas = b;
        }
        if let Some(s) = table.get("font_fallback").and_then(|v| v.as_str()) {
            cfg.font_fallback = s.to_string();
        }
        cfg
    }

    /// GAP-60: `[pane_capture]` —— 面板捕获
    pub fn load_pane_capture(path: &Path) -> crate::pane_capture::PaneCaptureConfig {
        let mut cfg = crate::pane_capture::PaneCaptureConfig::default();
        let Some(table) = Self::load_table(path, "pane_capture") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("default_format").and_then(|v| v.as_str()) {
            cfg.default_format = s.to_string();
        }
        cfg
    }

    /// GAP-61: `[print]` —— 打印支持
    pub fn load_print_support(path: &Path) -> crate::print_support::PrintSupportConfig {
        let mut cfg = crate::print_support::PrintSupportConfig::default();
        let Some(table) = Self::load_table(path, "print") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("include_header").and_then(|v| v.as_bool()) {
            cfg.include_header = b;
        }
        if let Some(b) = table.get("include_footer").and_then(|v| v.as_bool()) {
            cfg.include_footer = b;
        }
        cfg
    }

    /// GAP-4.6: `[accessibility]` —— 无障碍
    pub fn load_accessibility(path: &Path) -> crate::accessibility::AccessibilityConfig {
        let mut cfg = crate::accessibility::AccessibilityConfig::default();
        let Some(table) = Self::load_table(path, "accessibility") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("announce_tab_changes").and_then(|v| v.as_bool()) {
            cfg.announce_tab_changes = b;
        }
        if let Some(b) = table.get("announce_notifications").and_then(|v| v.as_bool()) {
            cfg.announce_notifications = b;
        }
        cfg
    }
}
