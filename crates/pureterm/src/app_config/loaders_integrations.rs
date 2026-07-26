//! `load_*` 方法 —— 集成/外部协议相关的配置加载器。
//!
//! 本文件是 `impl AppConfig` 的拆分块,包含与 MCP、AppleScript、URL Scheme、
//! LSP、快捷键(`[[keyboard.bindings]]`)、自定义命令(`[[commands]]`)、
//! 触发器(`[[triggers]]`)相关的静态加载方法。

use std::path::Path;

use crate::keymap::{CustomCommand, RawBinding};
use crate::triggers::{CompiledTrigger, TriggerConfig};
use super::{AppleScriptConfig, LspConfig, McpConfig, UrlSchemeConfig};
use super::parse_helpers::parse_trigger_action;

use super::AppConfig;

impl AppConfig {
    /// Load the `[mcp]` table (GAP-7.10). A missing/malformed table yields
    /// `Default` (all tools, 5000-line capture cap). See `McpConfig`.
    pub fn load_mcp(path: &Path) -> McpConfig {
        let mut cfg = McpConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("mcp").and_then(|m| m.as_table()) else {
            return cfg;
        };
        if let Some(n) = table.get("default_capture_lines").and_then(|v| v.as_integer()) {
            // Floor at 1 — a 0/negative cap would make capture_pane useless.
            cfg.default_capture_lines = (n.max(1)) as usize;
        }
        if let Some(arr) = table.get("enabled_tools").and_then(|v| v.as_array()) {
            cfg.enabled_tools = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        cfg
    }

    /// GAP-47: load `[applescript]` table.
    pub fn load_applescript(path: &Path) -> AppleScriptConfig {
        let mut cfg = AppleScriptConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("applescript").and_then(|m| m.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        cfg
    }

    /// GAP-48: load `[url_scheme]` table.
    pub fn load_url_scheme(path: &Path) -> UrlSchemeConfig {
        let mut cfg = UrlSchemeConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("url_scheme").and_then(|m| m.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("register_on_launch").and_then(|v| v.as_bool()) {
            cfg.register_on_launch = b;
        }
        cfg
    }

    /// Load the `[lsp]` table (GAP-35). A missing/malformed table yields
    /// `Default` (`enabled = false`). `[lsp.servers]` maps language ids to
    /// server binaries; MVP reads `rust` only.
    pub fn load_lsp(path: &Path) -> LspConfig {
        let mut cfg = LspConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("lsp").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(servers) = table.get("servers").and_then(|v| v.as_table()) {
            for (lang, cmd) in servers {
                if let Some(s) = cmd.as_str() {
                    let s = s.trim();
                    if !s.is_empty() {
                        cfg.servers.insert(lang.clone(), s.to_string());
                    }
                }
            }
        }
        cfg
    }

    /// Load `[[keyboard.bindings]]` from the TOML config.
    ///
    /// Each entry has `key`, `mods` (optional), and `action` fields in
    /// Alacritty-compatible format. Returns raw strings for the keymap
    /// module to parse into typed `Binding` structs.
    ///
    /// Failure-tolerant: missing or malformed bindings are silently
    /// skipped; an empty Vec is returned if the table is absent.
    pub fn load_keybindings(path: &Path) -> Vec<RawBinding> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_keybindings: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_keybindings: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        let Some(bindings_value) = value
            .get("keyboard")
            .and_then(|k| k.get("bindings"))
            .and_then(|b| b.as_array())
        else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for entry in bindings_value {
            let table = match entry.as_table() {
                Some(t) => t,
                None => continue,
            };
            let key = match table.get("key").and_then(|v| v.as_str()) {
                Some(k) => k.to_string(),
                None => continue,
            };
            let mods = table
                .get("mods")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let action = match table.get("action").and_then(|v| v.as_str()) {
                Some(a) => a.to_string(),
                None => continue,
            };
            result.push(RawBinding { key, mods, action });
        }
        if !result.is_empty() {
            log::info!("load_keybindings: loaded {} bindings", result.len());
        }
        result
    }

    /// GAP-6.4: Load the user's `[[commands]]` array from the toml config.
    ///
    /// Each entry is `{ id, title, command, key?, mods? }`:
    /// ```toml
    /// [[commands]]
    /// id = "deploy.staging"
    /// title = "Deploy to Staging"
    /// key = "Cmd+Shift+D"
    /// command = "ssh staging 'cd app && ./deploy.sh'"
    /// ```
    ///
    /// Failure-tolerant, mirroring `load_keybindings`: a missing file or
    /// `[[commands]]` table returns an empty Vec; entries missing required
    /// fields are skipped (with a `warn`). Duplicate `id`s collapse to the
    /// last definition (later wins).
    pub fn load_custom_commands(path: &Path) -> Vec<CustomCommand> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_custom_commands: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_custom_commands: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        let Some(arr) = value.get("commands").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        let mut result: Vec<CustomCommand> = Vec::new();
        for entry in arr {
            let table = match entry.as_table() {
                Some(t) => t,
                None => continue,
            };
            let id = match table.get("id").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!("load_custom_commands: entry missing 'id', skipped");
                    continue;
                },
            };
            let title = match table.get("title").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_custom_commands: entry '{}' missing 'title', skipped",
                        id
                    );
                    continue;
                },
            };
            let command = match table.get("command").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_custom_commands: entry '{}' missing 'command', skipped",
                        id
                    );
                    continue;
                },
            };
            let key = table
                .get("key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mods = table
                .get("mods")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let cmd = CustomCommand {
                id,
                title,
                command,
                key,
                mods,
            };
            // Dedup by id: later definition overrides an earlier one.
            if let Some(existing) = result.iter_mut().find(|e| e.id == cmd.id) {
                log::warn!(
                    "load_custom_commands: duplicate id '{}' — later entry overrides",
                    cmd.id
                );
                *existing = cmd;
            } else {
                result.push(cmd);
            }
        }
        if !result.is_empty() {
            log::info!("load_custom_commands: loaded {} commands", result.len());
        }
        result
    }

    /// GAP-10: Load the user's `[[triggers]]` array from the toml config.
    ///
    /// Each entry compiles a regex `pattern` matched against terminal output
    /// and fires one `action` on match. Supported actions: `highlight`,
    /// `notify`, `send_text`, `exec`, `set_variable`, `set_mark`, `fold`.
    /// `command`/`args`/`text`/`value`/`title` are template-expanded before
    /// dispatch (`{match}`, `$1`..`$9`, `${var:NAME}`, `\n\r\t\xNN`):
    /// ```toml
    /// [[triggers]]
    /// name = "error-spot"
    /// pattern = "(?i)error:|panic:"
    /// action = "highlight"          # optional: color = "#ff5555"
    ///
    /// [[triggers]]
    /// name = "sudo-reply"
    /// pattern = "\\[sudo\\] password"
    /// action = "send_text"
    /// text = "mypassword\r"         # optional: pane_id = 3 (default = matching pane)
    ///
    /// [[triggers]]
    /// name = "build-done"
    /// pattern = "Built in \\d+s"
    /// action = "notify"             # optional: title = "Build complete"
    ///
    /// [[triggers]]
    /// name = "open-link"
    /// pattern = "https://example\\.com/(\\S+)"
    /// action = "exec"
    /// command = "open"
    /// args = ["$1"]                 # templates expanded: $1 = capture group 1
    /// route = "toast"               # "toast" (default) | "pane"
    /// ```
    ///
    /// Failure-tolerant, mirroring `load_custom_commands`: a missing file or
    /// `[[triggers]]` table returns an empty Vec; entries missing a required
    /// field (`name`/`pattern`/`action`) or with an unparseable regex are
    /// skipped (with a `warn`) — config is untrusted and must never panic the
    /// app. Duplicate `name`s collapse to the last definition (later wins).
    pub fn load_triggers(path: &Path) -> Vec<CompiledTrigger> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_triggers: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_triggers: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        let Some(arr) = value.get("triggers").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        let mut result: Vec<CompiledTrigger> = Vec::new();
        for entry in arr {
            let table = match entry.as_table() {
                Some(t) => t,
                None => continue,
            };
            let name = match table.get("name").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!("load_triggers: entry missing 'name', skipped");
                    continue;
                },
            };
            let pattern = match table.get("pattern").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!("load_triggers: entry '{}' missing 'pattern', skipped", name);
                    continue;
                },
            };
            let re = match regex::Regex::new(&pattern) {
                Ok(re) => re,
                Err(e) => {
                    log::warn!(
                        "load_triggers: entry '{}' has unparseable regex {:?}: {} — skipped",
                        name,
                        pattern,
                        e
                    );
                    continue;
                },
            };
            let action_str = match table.get("action").and_then(|v| v.as_str()) {
                Some(a) => a,
                None => {
                    log::warn!("load_triggers: entry '{}' missing 'action', skipped", name);
                    continue;
                },
            };
            let action = match parse_trigger_action(action_str, table, &name) {
                Some(a) => a,
                None => continue,
            };
            let cfg = TriggerConfig { name: name.clone(), pattern, action };
            let compiled = CompiledTrigger { cfg, re };
            // Dedup by name: later definition overrides an earlier one.
            if let Some(existing) = result.iter_mut().find(|e| e.cfg.name == name) {
                log::warn!(
                    "load_triggers: duplicate name '{}' — later entry overrides",
                    name
                );
                *existing = compiled;
            } else {
                result.push(compiled);
            }
        }
        if !result.is_empty() {
            log::info!("load_triggers: loaded {} triggers", result.len());
        }
        result
    }
}
