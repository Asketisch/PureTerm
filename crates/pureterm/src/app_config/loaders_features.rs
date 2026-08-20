//! `load_*` 方法 —— 进阶特性相关的配置加载器。
//!
//! 本文件是 `impl AppConfig` 的拆分块,包含环境 agent、广播输入、BiDi 模式、
//! workspace 分组、工具带、quake dock、语音输入、终端图形、shaper 等特性
//! 子配置的静态加载方法。从 `loaders_ui.rs` 拆出以控制单文件行数。

use std::path::Path;

use super::AppConfig;
use super::{BlockOutputConfig, NotificationConfig, SemanticHistoryConfig};

impl AppConfig {
    pub fn load_ambient_agents(path: &Path) -> crate::ambient_agents::AmbientAgentsConfig {
        let mut cfg = crate::ambient_agents::AmbientAgentsConfig::default();
        let Some(table) = Self::load_table(path, "ambient_agents") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(n) = table.get("poll_interval_secs").and_then(|v| v.as_integer()) {
            cfg.poll_interval_secs = (n.max(1)) as u64;
        }
        cfg
    }

    /// GAP-13: `[broadcast_input]` —— 广播输入
    pub fn load_broadcast_input(path: &Path) -> crate::broadcast_input::BroadcastConfig {
        let mut cfg = crate::broadcast_input::BroadcastConfig::default();
        let Some(table) = Self::load_table(path, "broadcast_input") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        cfg
    }

    /// GAP-18: `[terminal].bidi_mode` —— 双向文本模式
    pub fn load_bidi_mode(path: &Path) -> crate::bidi::BidiMode {
        let Some(table) = Self::load_table(path, "terminal") else {
            return crate::bidi::BidiMode::default();
        };
        table
            .get("bidi_mode")
            .and_then(|v| v.as_str())
            .map(crate::bidi::BidiMode::parse)
            .unwrap_or_default()
    }

    /// GAP-26: `[[workspace_groups]]` —— 工作区分组
    pub fn load_workspace_groups(path: &Path) -> Vec<crate::workspace_groups::WorkspaceGroup> {
        #[derive(serde::Deserialize)]
        struct Wrap {
            #[serde(default)]
            workspace_groups: Vec<crate::workspace_groups::WorkspaceGroup>,
        }
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        toml::from_str::<Wrap>(&contents)
            .map(|w| w.workspace_groups)
            .unwrap_or_default()
    }

    /// GAP-30: `[toolbelt]` —— 工具带
    pub fn load_toolbelt(path: &Path) -> crate::toolbelt::ToolbeltConfig {
        let mut cfg = crate::toolbelt::ToolbeltConfig::default();
        let Some(table) = Self::load_table(path, "toolbelt") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(arr) = table.get("tools").and_then(|v| v.as_array()) {
            cfg.tools = arr
                .iter()
                .filter_map(|v| v.as_str().and_then(crate::toolbelt::ToolbeltTool::parse))
                .collect();
        }
        cfg
    }

    /// GAP-31: `[quake]` —— quake dock 下拉窗
    pub fn load_quake(path: &Path) -> crate::quake_dock::QuakeDockConfig {
        let mut cfg = crate::quake_dock::QuakeDockConfig::default();
        let Some(table) = Self::load_table(path, "quake") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(n) = table.get("height_fraction").and_then(|v| v.as_float()) {
            cfg.height_fraction = n as f32;
        }
        cfg
    }

    // ── P4 阶段 GAP 配置加载器 ─────────────────────────────────────────────────────

    /// GAP-05: `[voice_input]` —— 语音输入
    pub fn load_voice_input(path: &Path) -> crate::voice_input::VoiceInputConfig {
        let mut cfg = crate::voice_input::VoiceInputConfig::default();
        let Some(table) = Self::load_table(path, "voice_input") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("api_url").and_then(|v| v.as_str()) {
            cfg.api_url = s.to_string();
        }
        if let Some(b) = table.get("push_to_talk").and_then(|v| v.as_bool()) {
            cfg.push_to_talk = b;
        }
        cfg
    }

    /// GAP-17: `[terminal_graphics]`。T-core-closure 新增 `max_image_bytes`
    /// 与 `max_concurrent_images`,使病态的超大分块图
    /// 形 PNG 负载(数 MB)在钉住
    /// 存储之前,于配置层即被拒绝。
    pub fn load_terminal_graphics(path: &Path) -> crate::terminal_graphics::TerminalGraphicsConfig {
        let mut cfg = crate::terminal_graphics::TerminalGraphicsConfig::default();
        let Some(table) = Self::load_table(path, "terminal_graphics") else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(n) = table.get("max_images").and_then(|v| v.as_integer()) {
            cfg.max_images = (n.max(1)) as usize;
        }
        if let Some(n) = table.get("max_image_bytes").and_then(|v| v.as_integer()) {
            cfg.max_image_bytes = (n.max(1024)) as usize;
        }
        cfg
    }

    /// T-core-shaper v2:`[shaper]` 配置块——后端、语言、回退链。
    ///
    /// 真正的 TOML 解析在 `shaper_config::ShaperConfig::load` 中完成,
    /// 此处仅为易用性提供一个薄封装。
    pub fn load_shaper(path: &Path) -> crate::shaper_config::ShaperConfig {
        crate::shaper_config::ShaperConfig::load(path)
    }

    /// 加载 `[semantic_history]` 表(GAP-36)。
    pub fn load_semantic_history(path: &Path) -> SemanticHistoryConfig {
        let mut cfg = SemanticHistoryConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("semantic_history").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(s) = table.get("editor").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.editor = s.to_string();
            }
        }
        cfg
    }

    /// 加载 `[block_output]` 表(GAP-51 脚手架)。
    pub fn load_block_output(path: &Path) -> BlockOutputConfig {
        let mut cfg = BlockOutputConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("block_output").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        cfg
    }

    /// 加载 `[notification]` 表(GAP-09)。缺失或格式错误的表
    /// 回退为 `Default`。与 `load_git` 保持一致:read_to_string → toml::from_str →
    /// 遍历 `value.get("notification").as_table()`;每个键均独立
    /// 可选。
    pub fn load_notification(path: &Path) -> NotificationConfig {
        let mut cfg = NotificationConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("notification").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("desktop_notifications").and_then(|v| v.as_bool()) {
            cfg.desktop_notifications = b;
        }
        if let Some(b) = table.get("dock_badge").and_then(|v| v.as_bool()) {
            cfg.dock_badge = b;
        }
        if let Some(b) = table.get("ring").and_then(|v| v.as_bool()) {
            cfg.ring = b;
        }
        if let Some(b) = table.get("bell_passthrough").and_then(|v| v.as_bool()) {
            cfg.bell_passthrough = b;
        }
        if let Some(n) = table.get("bell_throttle_ms").and_then(|v| v.as_integer()) {
            // 限制为 ≥0(0 表示不节流;不会除零,因为
            // 存储比较的是墙钟毫秒数,而非取模)。
            cfg.bell_throttle_ms = n.max(0) as u64;
        }
        cfg
    }
}
