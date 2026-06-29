//! GAP-3.2: Programmable Sidebar DSL — minimal TOML interpreter + EventBus hook.
//!
//! Sidebar scripts live in `~/.config/pureterm/sidebars/*.toml`. Each file
//! declares event triggers and renders `SidebarItem` rows when matching events
//! arrive on the in-process EventBus.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::events::{AppEvent, Category};

/// One rendered row in the programmable sidebar panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarItem {
    pub icon: String,
    pub text: String,
    pub action: Option<String>,
}

/// Parsed sidebar script (minimal DSL — no Rhai yet).
#[derive(Debug, Clone)]
pub struct SidebarScript {
    pub name: String,
    pub subscribe: Vec<String>,
    pub template: String,
    pub icon: String,
    pub action: Option<String>,
}

/// Runtime state: aggregated items keyed by script name.
#[derive(Debug, Clone, Default)]
pub struct SidebarDslState {
    pub items: Vec<SidebarItem>,
    scripts: Vec<SidebarScript>,
}

impl SidebarDslState {
    pub fn load_from_dir(dir: &Path) -> Self {
        let mut scripts = Vec::new();
        if dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                        if let Ok(text) = std::fs::read_to_string(&path) {
                            if let Ok(script) = parse_sidebar_script(&text, &path) {
                                scripts.push(script);
                            }
                        }
                    }
                }
            }
        }
        Self {
            items: Vec::new(),
            scripts,
        }
    }

    /// Handle one EventBus event — update matching script rows.
    pub fn on_event(&mut self, event: &AppEvent) {
        self.items.clear();
        for script in &self.scripts {
            if !script_matches(script, event) {
                continue;
            }
            let text = render_template(&script.template, event);
            self.items.push(SidebarItem {
                icon: script.icon.clone(),
                text,
                action: script.action.clone(),
            });
        }
    }

    pub fn script_count(&self) -> usize {
        self.scripts.len()
    }
}

fn script_matches(script: &SidebarScript, event: &AppEvent) -> bool {
    if script.subscribe.is_empty() {
        return false;
    }
    for pat in &script.subscribe {
        if pat.ends_with('*') {
            let prefix = pat.trim_end_matches('*');
            if event.kind.starts_with(prefix) || event.cat.as_str().starts_with(prefix) {
                return true;
            }
        } else if pat.contains('.') {
            if event.kind == *pat {
                return true;
            }
        } else if event.cat.as_str() == pat {
            return true;
        }
    }
    false
}

fn render_template(tmpl: &str, event: &AppEvent) -> String {
    let mut out = tmpl.to_string();
    out = out.replace("{kind}", &event.kind);
    out = out.replace("{cat}", event.cat.as_str());
    if let Some(sid) = &event.session_id {
        out = out.replace("{session_id}", sid);
    }
    if let Some(v) = event.data.get("command").and_then(|v| v.as_str()) {
        out = out.replace("{command}", v);
    }
    if let Some(v) = event.data.get("workspace").and_then(|v| v.as_str()) {
        out = out.replace("{workspace}", v);
    }
    if let Some(v) = event.data.get("branch").and_then(|v| v.as_str()) {
        out = out.replace("{branch}", v);
    }
    out
}

/// Parse one sidebar TOML file.
pub fn parse_sidebar_script(text: &str, path: &Path) -> Result<SidebarScript, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let name = table
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("sidebar")
                .to_string()
        });
    let subscribe = parse_subscribe(table.get("subscribe"));
    let template = table
        .get("template")
        .and_then(|v| v.as_str())
        .unwrap_or("{kind}")
        .to_string();
    let icon = table
        .get("icon")
        .and_then(|v| v.as_str())
        .unwrap_or("•")
        .to_string();
    let action = table
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Ok(SidebarScript {
        name,
        subscribe,
        template,
        icon,
        action,
    })
}

fn parse_subscribe(v: Option<&toml::Value>) -> Vec<String> {
    match v {
        Some(toml::Value::String(s)) => vec![s.clone()],
        Some(toml::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => vec!["shell.*".to_string()],
    }
}

pub fn sidebars_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/pureterm/sidebars")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_show_git_style_script() {
        let text = r#"
name = "show-git"
subscribe = ["git.*"]
template = "branch: {branch}"
icon = "G"
"#;
        let script = parse_sidebar_script(text, Path::new("show-git.toml")).unwrap();
        assert_eq!(script.subscribe, vec!["git.*"]);
        assert!(script.template.contains("{branch}"));
    }

    #[test]
    fn event_updates_items() {
        let text = r#"
subscribe = "shell.command_finished"
template = "exit at {kind}"
"#;
        let mut state = SidebarDslState {
            scripts: vec![parse_sidebar_script(text, Path::new("t.toml")).unwrap()],
            items: Vec::new(),
        };
        let ev = AppEvent {
            seq: 1,
            ts: 0,
            cat: Category::Shell,
            kind: "shell.command_finished".into(),
            session_id: None,
            data: json!({}),
        };
        state.on_event(&ev);
        assert_eq!(state.items.len(), 1);
        assert!(state.items[0].text.contains("shell.command_finished"));
    }

    #[test]
    fn wildcard_subscribe_matches_category() {
        let script = SidebarScript {
            name: "t".into(),
            subscribe: vec!["shell.*".into()],
            template: "{kind}".into(),
            icon: "•".into(),
            action: None,
        };
        let ev = AppEvent {
            seq: 1,
            ts: 0,
            cat: Category::Shell,
            kind: "shell.prompt_started".into(),
            session_id: None,
            data: Value::Null,
        };
        assert!(script_matches(&script, &ev));
    }
}
