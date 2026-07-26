//! `load_*` 方法 —— UI / 终端表现相关的配置加载器。
//!
//! 本文件是 `impl AppConfig` 的拆分块,包含状态栏、Git 集成、侧栏、Shell 集成、
//! 错误恢复、AI 聊天、灵魂模式、自然语言、端口扫描、环境 agent、广播输入、
//! BiDi 模式、workspace 分组、工具带、quake dock、语音输入、终端图形、shaper、
//! 语义历史、块输出、通知等 UI 相关子配置的静态加载方法。

use std::path::{Path, PathBuf};

use super::{
    AiChatConfig, BlockOutputConfig, ErrorRecoveryConfig, GitConfig, NaturalLanguageConfig,
    NotificationConfig, PortScannerConfig, SemanticHistoryConfig, ShellIntegrationConfig,
    SidebarConfig, SoulConfig, StatusBarConfig,
};

use super::AppConfig;

impl AppConfig {
    /// Load the `[statusbar]` table (GAP-24). A missing/malformed table yields
    /// `Default`. Mirrors `load_mcp`: read_to_string → toml::from_str → walk
    /// `value.get("statusbar").as_table()`; each key is independently optional.
    pub fn load_statusbar(path: &Path) -> StatusBarConfig {
        let mut cfg = StatusBarConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("statusbar").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(arr) = table.get("components").and_then(|v| v.as_array()) {
            let ids: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            // Only override defaults if the list is non-empty — an empty array
            // would render a blank bar, which is almost certainly a config mistake.
            if !ids.is_empty() {
                cfg.components = ids;
            }
        }
        if let Some(n) = table.get("refresh_interval_secs").and_then(|v| v.as_integer()) {
            cfg.refresh_interval_secs = (n.max(1)) as u64;
        }
        if let Some(b) = table.get("show_sparklines").and_then(|v| v.as_bool()) {
            cfg.show_sparklines = b;
        }
        if let Some(h) = table.get("bar_height").and_then(|v| v.as_integer()) {
            // Clamp to a sane readable range (18px minimum, 48px maximum).
            cfg.bar_height = h.clamp(18, 48) as u16;
        }
        if let Some(s) = table.get("clock_format").and_then(|v| v.as_str()) {
            cfg.clock_format = s.to_string();
        }
        if let Some(b) = table.get("clock_show_seconds").and_then(|v| v.as_bool()) {
            cfg.clock_show_seconds = b;
        }
        cfg
    }

    /// Load the `[git]` table (GAP-32). A missing/malformed table yields
    /// `Default`. Mirrors `load_statusbar`: read_to_string → toml::from_str →
    /// walk `value.get("git").as_table()`; each key is independently optional.
    pub fn load_git(path: &Path) -> GitConfig {
        let mut cfg = GitConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("git").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("panel_enabled").and_then(|v| v.as_bool()) {
            cfg.panel_enabled = b;
        }
        if let Some(b) = table.get("show_pr").and_then(|v| v.as_bool()) {
            cfg.show_pr = b;
        }
        if let Some(n) = table.get("pr_refresh_ticks").and_then(|v| v.as_integer()) {
            // Clamp to ≥1 so the modulo throttle never divides by zero.
            cfg.pr_refresh_ticks = (n.max(1)) as u64;
        }
        if let Some(s) = table.get("lazygit_cmd").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.lazygit_cmd = Some(s.to_string());
            }
        }
        if let Some(b) = table.get("tab_title_branch").and_then(|v| v.as_bool()) {
            cfg.tab_title_branch = b;
        }
        cfg
    }

    /// Load the `[sidebar]` table (GAP-19). A missing/malformed table yields
    /// `Default`. Width keys are clamped to 120–480 px.
    pub fn load_sidebar(path: &Path) -> SidebarConfig {
        let mut cfg = SidebarConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("sidebar").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(n) = table.get("left_width").and_then(|v| v.as_float()) {
            cfg.left_width = crate::sidebar::clamp_width(n as f32);
        } else if let Some(n) = table.get("left_width").and_then(|v| v.as_integer()) {
            cfg.left_width = crate::sidebar::clamp_width(n as f32);
        }
        if let Some(n) = table.get("right_width").and_then(|v| v.as_float()) {
            cfg.right_width = crate::sidebar::clamp_width(n as f32);
        } else if let Some(n) = table.get("right_width").and_then(|v| v.as_integer()) {
            cfg.right_width = crate::sidebar::clamp_width(n as f32);
        }
        if let Some(b) = table.get("left_default_open").and_then(|v| v.as_bool()) {
            cfg.left_default_open = b;
        }
        cfg
    }

    /// Load the `[shell_integration]` table (GAP-15). A missing/malformed table
    /// yields `Default`. Mirrors `load_git`: read_to_string → toml::from_str →
    /// walk `value.get("shell_integration").as_table()`; each key is
    /// independently optional.
    pub fn load_shell_integration(path: &Path) -> ShellIntegrationConfig {
        let mut cfg = ShellIntegrationConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("shell_integration").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("inject_zsh").and_then(|v| v.as_bool()) {
            cfg.inject_zsh = b;
        }
        if let Some(b) = table.get("inject_bash").and_then(|v| v.as_bool()) {
            cfg.inject_bash = b;
        }
        if let Some(b) = table.get("include_command_text").and_then(|v| v.as_bool()) {
            cfg.include_command_text = b;
        }
        // t-core-shell-integ: per-shell opt-in map. Keys are
        // `[shell_integration.per_shell.<shell_name>] = true|false`; the
        // exact recognized names are the lowercase enum variants
        // (`zsh`/`bash`/`fish`/`nu`/`elvish`/`pwsh`).
        if let Some(per_shell) = table.get("per_shell").and_then(|t| t.as_table()) {
            for (k, v) in per_shell {
                if let Some(b) = v.as_bool() {
                    cfg.per_shell.insert(k.clone(), b);
                }
            }
        }
        cfg
    }

    /// Load the `[error_recovery]` table (GAP-02). A missing/malformed table
    /// yields `Default`. Mirrors `load_notification`.
    pub fn load_error_recovery(path: &Path) -> ErrorRecoveryConfig {
        let mut cfg = ErrorRecoveryConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("error_recovery").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("auto_analyze").and_then(|v| v.as_bool()) {
            cfg.auto_analyze = b;
        }
        if let Some(s) = table.get("provider").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.provider = s.to_string();
            }
        }
        if let Some(s) = table.get("model").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.model = s.to_string();
            }
        }
        if let Some(n) = table.get("max_output_lines").and_then(|v| v.as_integer()) {
            cfg.max_output_lines = (n.max(1)) as usize;
        }
        if let Some(n) = table.get("max_tokens").and_then(|v| v.as_integer()) {
            cfg.max_tokens = (n.max(1)) as u32;
        }
        if let Some(s) = table.get("api_key_env").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.api_key_env = Some(s.to_string());
            }
        }
        if let Some(s) = table.get("openai_base_url").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.openai_base_url = Some(s.to_string());
            }
        }
        cfg
    }

    /// Load the `[ai_chat]` table (GAP-01). A missing/malformed table yields
    /// `Default`. Mirrors `load_error_recovery`.
    pub fn load_ai_chat(path: &Path) -> AiChatConfig {
        let mut cfg = AiChatConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("ai_chat").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(s) = table.get("provider").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.provider = s.to_string();
            }
        }
        if let Some(s) = table.get("model").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.model = s.to_string();
            }
        }
        if let Some(n) = table.get("max_tokens").and_then(|v| v.as_integer()) {
            cfg.max_tokens = (n.max(1)) as u32;
        }
        if let Some(s) = table.get("api_key_env").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.api_key_env = Some(s.to_string());
            }
        }
        if let Some(s) = table.get("openai_base_url").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.openai_base_url = Some(s.to_string());
            }
        }
        cfg
    }

    /// Load the `[soul]` table (GAP-07).
    pub fn load_soul(path: &Path) -> SoulConfig {
        let mut cfg = SoulConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("soul").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("directory").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.directory = Some(PathBuf::from(s));
            }
        }
        cfg
    }

    /// Load the `[natural_language]` table (GAP-08).
    pub fn load_natural_language(path: &Path) -> NaturalLanguageConfig {
        let mut cfg = NaturalLanguageConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("natural_language").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(n) = table.get("max_tokens").and_then(|v| v.as_integer()) {
            cfg.max_tokens = (n.max(1)) as u32;
        }
        cfg
    }

    /// Load the `[port_scanner]` table (GAP-33).
    pub fn load_port_scanner(path: &Path) -> PortScannerConfig {
        let mut cfg = PortScannerConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("port_scanner").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(n) = table.get("interval").and_then(|v| v.as_integer()) {
            cfg.interval_secs = (n.max(1)) as u64;
        } else if let Some(n) = table.get("interval_secs").and_then(|v| v.as_integer()) {
            cfg.interval_secs = (n.max(1)) as u64;
        }
        if let Some(n) = table.get("min_port").and_then(|v| v.as_integer()) {
            cfg.min_port = (n.max(0)) as u16;
        }
        cfg
    }
}
