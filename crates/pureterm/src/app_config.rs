//! PureTerm 主配置 —— `AppConfig` 主结构体 + 核心 load/config_path/load_table,
//! 以及子模块声明与 re-export。
//!
//! 模块布局:
//! - `structs_core` / `structs_ui`: 各域子配置结构体
//! - `loaders_agent` / `loaders_integrations` / `loaders_ui` / `loaders_features`
//!   / `loaders_misc`: `impl AppConfig` 的 `load_*` 静态方法(按域拆分)
//! - `parse_helpers`: `parse_hex_color` / `parse_trigger_action`
//! - `tests`: 单元测试(`#[path]` 引入,不计入主文件行数)

use std::path::{Path, PathBuf};

use pureterm_settings::config_io;

mod loaders_agent;
mod loaders_features;
mod loaders_integrations;
mod loaders_misc;
mod loaders_ui;
mod parse_helpers;
mod structs_core;
mod structs_ui;

// 子配置结构体 re-export(外部按 `app_config::XxxConfig` 引用)。
pub use structs_core::{
    AgentDetectConfig, AgentHibernationConfig, AppleScriptConfig, LspConfig, McpConfig,
    ResumeBinding, UrlSchemeConfig, VaultAgentEntry,
};
pub use structs_ui::{
    AiChatConfig, AppearanceConfig, BlockOutputConfig, ErrorRecoveryConfig, GitConfig,
    NaturalLanguageConfig, NotificationConfig, PortScannerConfig, SemanticHistoryConfig,
    ShellIntegrationConfig, SidebarConfig, SoulConfig, StatusBarConfig,
};

// parse_helpers 的函数仅在 app_config 模块树内部使用,不对外 re-export。
use parse_helpers::parse_hex_color;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub font_family: String,
    pub font_size: f32,
    pub background_color: [f32; 4],
    pub foreground_color: [f32; 4],
    pub opacity: f32,
    pub locale: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font_family: "monospace".to_string(),
            font_size: 14.0,
            background_color: [0.11, 0.11, 0.12, 1.0],
            foreground_color: [0.83, 0.84, 0.81, 1.0],
            opacity: 1.0,
            locale: "en".to_string(),
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        let mut config = Self::default();

        let doc = match config_io::load_document(path) {
            Ok(doc) => doc,
            Err(_) => return config,
        };

        if let Some(family) = config_io::get_string(&doc, &["font", "normal"], "family") {
            if !family.is_empty() {
                config.font_family = family;
            }
        }

        if let Some(size) = config_io::get_float(&doc, &["font"], "size") {
            if size > 0.0 {
                config.font_size = size as f32;
            }
        }

        if let Some(bg) = config_io::get_string(&doc, &["colors", "primary"], "background") {
            if let Some(color) = parse_hex_color(&bg) {
                config.background_color = color;
            }
        }

        if let Some(fg) = config_io::get_string(&doc, &["colors", "primary"], "foreground") {
            if let Some(color) = parse_hex_color(&fg) {
                config.foreground_color = color;
            }
        }

        if let Some(opacity) = config_io::get_float(&doc, &["window"], "opacity") {
            config.opacity = opacity.clamp(0.0, 1.0) as f32;
        }

        if let Some(locale) = config_io::get_string(&doc, &["pureterm"], "locale") {
            if !locale.is_empty() {
                config.locale = locale;
            }
        }

        config
    }

    pub fn config_path() -> PathBuf {
        config_io::find_config_path().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".config/pureterm/pureterm.toml")
        })
    }

    fn load_table(path: &Path, key: &str) -> Option<toml::map::Map<String, toml::Value>> {
        let contents = std::fs::read_to_string(path).ok()?;
        let value: toml::Value = toml::from_str(&contents).ok()?;
        value.get(key).and_then(|t| t.as_table()).cloned()
    }
}

#[cfg(test)]
#[path = "app_config/tests.rs"]
mod tests;
