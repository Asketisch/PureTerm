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

    /// GAP-13: `[broadcast_input]`
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

    /// GAP-18: `[terminal].bidi_mode`
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

    /// GAP-26: `[[workspace_groups]]`
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

    /// GAP-30: `[toolbelt]`
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

    /// GAP-31: `[quake]`
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

    // ── P4 GAP config loaders ───────────────────────────────────────────

    /// GAP-05: `[voice_input]`
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

    /// GAP-17: `[terminal_graphics]`. T-core-closure adds `max_image_bytes`
    /// and `max_concurrent_images` so pathological Kitty PNG payloads
    /// (multi-MB) are rejected at the config level before they pin the
    /// store.
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

    /// T-core-shaper v2: `[shaper]` block — backend, language, fallback chain.
    ///
    /// The loader in `shaper_config::ShaperConfig::load` does the actual
    /// TOML parse; this is a thin alias for ergonomics.
    pub fn load_shaper(path: &Path) -> crate::shaper_config::ShaperConfig {
        crate::shaper_config::ShaperConfig::load(path)
    }

    /// Load the `[semantic_history]` table (GAP-36).
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

    /// Load the `[block_output]` table (GAP-51 scaffold).
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

    /// Load the `[notification]` table (GAP-09). A missing/malformed table
    /// yields `Default`. Mirrors `load_git`: read_to_string → toml::from_str →
    /// walk `value.get("notification").as_table()`; each key is independently
    /// optional.
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
            // Clamp to ≥0 (0 disables throttling, not divide-by-zero since the
            // store compares wall-clock ms, not a modulo).
            cfg.bell_throttle_ms = n.max(0) as u64;
        }
        cfg
    }
}
