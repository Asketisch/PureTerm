//! Trigger configuration for TOML deserialization.
//!
//! Usage in `alacritty.toml`:
//! ```toml
//! [[trigger]]
//! name = "Error Highlight"
//! pattern = "(?i)\\berror\\b"
//! action = { highlight = { color = "#FF0000" } }
//! enabled = true
//! ```

use serde::{Deserialize, Serialize};

use crate::trigger::{HighlightScope, TriggerAction, Trigger};

/// TOML-level trigger config (plain serde, not ConfigDeserialize).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct TriggerConfig {
    #[serde(default)]
    pub id: String,

    #[serde(default)]
    pub name: String,

    pub pattern: String,

    #[serde(default)]
    pub action: Option<toml::Value>,

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default)]
    pub case_sensitive: bool,
}

fn default_true() -> bool {
    true
}

/// Parse a `toml::Value` into a `TriggerAction`.
pub fn parse_action(value: &toml::Value) -> TriggerAction {
    match value {
        toml::Value::String(s) => match s.as_str() {
            "Bell" => TriggerAction::Bell,
            "CopyMatch" => TriggerAction::CopyMatch,
            "DirectoryTrack" => TriggerAction::DirectoryTrack,
            "PasswordMask" => TriggerAction::PasswordMask,
            _ => TriggerAction::Bell,
        },
        toml::Value::Table(map) => {
            if let Some(v) = map.get("highlight") {
                let color = v.get("color").and_then(|v| v.as_str()).unwrap_or("#FF0000").to_string();
                let scope = v.get("scope")
                    .and_then(|v| v.as_str())
                    .map(|s| if s == "WholeLine" { HighlightScope::WholeLine } else { HighlightScope::Match })
                    .unwrap_or(HighlightScope::Match);
                TriggerAction::Highlight { color, scope }
            } else if let Some(v) = map.get("notify") {
                let title = v.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let body = v.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
                TriggerAction::Notify { title, body }
            } else if let Some(v) = map.get("run_command") {
                let command = v.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string();
                TriggerAction::RunCommand { command }
            } else if let Some(v) = map.get("send_text") {
                let text = v.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let append_newline = v.get("append_newline").and_then(|v| v.as_bool()).unwrap_or(false);
                TriggerAction::SendText { text, append_newline }
            } else if let Some(v) = map.get("capture_to_file") {
                let path = v.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let max_bytes = v.get("max_bytes").and_then(|v| v.as_integer()).unwrap_or(100 * 1024 * 1024) as u64;
                TriggerAction::CaptureToFile { path, max_bytes }
            } else if let Some(v) = map.get("hyperlink") {
                let url_template = v.get("url_template").and_then(|v| v.as_str()).unwrap_or("").to_string();
                TriggerAction::Hyperlink { url_template }
            } else if let Some(v) = map.get("play_sound") {
                let sound = v.get("sound").and_then(|v| v.as_str()).map(|s| s.to_string());
                TriggerAction::PlaySound { sound }
            } else {
                TriggerAction::Bell
            }
        }
        _ => TriggerAction::Bell,
    }
}

/// Load triggers from the raw TOML config table.
pub fn load_triggers_from_table(table: &toml::Table) -> Vec<Trigger> {
    let Some(triggers_value) = table.get("trigger") else {
        return TriggerService::default_triggers();
    };
    let toml::Value::Array(arr) = triggers_value else {
        return TriggerService::default_triggers();
    };
    let mut configs = Vec::new();
    for item in arr {
        match item.clone().try_into::<TriggerConfig>() {
            Ok(c) => configs.push(c),
            Err(e) => log::warn!("failed to parse trigger config: {}", e),
        }
    }
    if configs.is_empty() {
        return TriggerService::default_triggers();
    }
    configs.iter().enumerate().map(|(i, c)| c.to_trigger(i)).collect()
}

use crate::trigger::TriggerService;

impl TriggerConfig {
    pub fn to_trigger(&self, index: usize) -> Trigger {
        let id = if self.id.is_empty() {
            format!("trigger_{}", index)
        } else {
            self.id.clone()
        };
        Trigger {
            id,
            name: self.name.clone(),
            pattern: self.pattern.clone(),
            action: self.action.as_ref().map(|v| parse_action(v)).unwrap_or(TriggerAction::Bell),
            enabled: self.enabled,
            case_sensitive: self.case_sensitive,
        }
    }
}
