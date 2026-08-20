//! 配置解析辅助函数 —— `parse_hex_color` 与 `parse_trigger_action`。
//!
//! 从主 `app_config.rs` 抽出,供 `load`(主加载)与各 `loaders_*` 子模块共用。
//! `pub(super)` 可见,仅在 `app_config` 模块树内部使用。

use crate::triggers::{ExecRoute, TriggerAction};

pub(super) fn parse_hex_color(hex: &str) -> Option<[f32; 4]> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
}

/// GAP-10:解析 `[[triggers]]` 条目的 `action` 及其特定于动作的
/// 参数(来自条目表)。缺失必填参数或动作未知时
/// 返回 `None`(已告警),以便 `load_triggers` 跳过该条目。
pub(super) fn parse_trigger_action(
    action: &str,
    table: &toml::Table,
    name: &str,
) -> Option<TriggerAction> {
    match action {
        "highlight" => {
            let color = table.get("color").and_then(|v| v.as_str()).and_then(parse_hex_color);
            Some(TriggerAction::Highlight { color })
        },
        "notify" => {
            let title = table.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
            Some(TriggerAction::Notify { title })
        },
        "send_text" => {
            let text = match table.get("text").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: send_text entry '{}' missing 'text', skipped",
                        name
                    );
                    return None;
                },
            };
            let pane_id = table
                .get("pane_id")
                .and_then(|v| v.as_integer())
                .map(|n| n as u64);
            Some(TriggerAction::SendText { text, pane_id })
        },
        "exec" => {
            let command = match table.get("command").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: exec entry '{}' missing 'command', skipped",
                        name
                    );
                    return None;
                },
            };
            let args = table
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let route = match table.get("route").and_then(|v| v.as_str()) {
                Some("pane") => ExecRoute::Pane,
                _ => ExecRoute::Toast,
            };
            Some(TriggerAction::Exec { command, args, route })
        },
        "set_variable" => {
            let var_name = match table
                .get("variable")
                .or_else(|| table.get("var_name"))
                .and_then(|v| v.as_str())
            {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: set_variable entry '{}' missing 'variable', skipped",
                        name
                    );
                    return None;
                },
            };
            let value = match table.get("value").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: set_variable entry '{}' missing 'value', skipped",
                        name
                    );
                    return None;
                },
            };
            Some(TriggerAction::SetVariable { name: var_name, value })
        },
        "set_mark" => {
            let mark = match table.get("mark").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: set_mark entry '{}' missing 'mark', skipped",
                        name
                    );
                    return None;
                },
            };
            Some(TriggerAction::SetMark { mark })
        },
        "fold" => {
            let mark = table.get("mark").and_then(|v| v.as_str()).map(|s| s.to_string());
            let lines_back = table
                .get("lines_back")
                .and_then(|v| v.as_integer())
                .map(|n| n as u32);
            Some(TriggerAction::Fold { mark, lines_back })
        },
        other => {
            log::warn!(
                "load_triggers: entry '{}' has unknown action {:?}, skipped",
                name,
                other
            );
            None
        },
    }
}
