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
    /// 加载 `[mcp]` 表（GAP-7.10）。缺失/畸形表返回 `Default`
    ///（全部工具、5000 行捕获上限）。参见 `McpConfig`。
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
            // 下限为 1 —— 0/负上限会让 capture_pane 无意义。
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

    /// GAP-47：加载 `[applescript]` 表。
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

    /// GAP-48：加载 `[url_scheme]` 表。
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

    /// 加载 `[lsp]` 表（GAP-35）。缺失/畸形表返回 `Default`
    ///（`enabled = false`）。`[lsp.servers]` 把语言 id 映射到服务端
    /// 二进制；MVP 只读取 `rust`。
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

    /// 从 TOML 配置加载 `[[keyboard.bindings]]`。
    ///
    /// 每条含 `key`、`mods`（可选）和 `action` 字段，采用与
    /// Alacritty 兼容的格式。返回原始字符串，由 keymap 模块解析成
    /// 类型化的 `Binding` 结构体。
    ///
    /// 失败容忍：缺失或畸形的绑定被静默跳过；表缺失时返回空 Vec。
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

    /// GAP-6.4：从 toml 配置加载用户的 `[[commands]]` 数组。
    ///
    /// 每条为 `{ id, title, command, key?, mods? }`：
    /// ```toml
    /// [[commands]]
    /// id = "deploy.staging"
    /// title = "Deploy to Staging"
    /// key = "Cmd+Shift+D"
    /// command = "ssh staging 'cd app && ./deploy.sh'"
    /// ```
    ///
    /// 失败容忍，镜像 `load_keybindings`：文件缺失或 `[[commands]]` 表
    /// 缺失时返回空 Vec；缺必填字段的条目被跳过（带 `warn`）。重复的
    /// `id` 收敛到最后一个定义（后者胜出）。
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
            // 按 id 去重：后定义覆盖先定义。
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

    /// GAP-10：从 toml 配置加载用户的 `[[triggers]]` 数组。
    ///
    /// 每条编译一个正则 `pattern`，对终端输出匹配，命中后触发一个
    /// `action`。支持的动作：`highlight`、`notify`、`send_text`、
    /// `exec`、`set_variable`、`set_mark`、`fold`。`command`/`args`/
    /// `text`/`value`/`title` 在分发前做模板展开
    ///（`{match}`、`$1`..`$9`、`${var:NAME}`、`\n\r\t\xNN`）：
    /// ```toml
    /// [[triggers]]
    /// name = "error-spot"
    /// pattern = "(?i)error:|panic:"
    /// action = "highlight"          # 可选：color = "#ff5555"
    ///
    /// [[triggers]]
    /// name = "sudo-reply"
    /// pattern = "\\[sudo\\] password"
    /// action = "send_text"
    /// text = "mypassword\r"         # 可选：pane_id = 3（默认 = 匹配的面板）
    ///
    /// [[triggers]]
    /// name = "build-done"
    /// pattern = "Built in \\d+s"
    /// action = "notify"             # 可选：title = "构建完成"
    ///
    /// [[triggers]]
    /// name = "open-link"
    /// pattern = "https://example\\.com/(\\S+)"
    /// action = "exec"
    /// command = "open"
    /// args = ["$1"]                 # 模板展开：$1 = 捕获组 1
    /// route = "toast"               # "toast"（默认）| "pane"
    /// ```
    ///
    /// 失败容忍，镜像 `load_custom_commands`：文件缺失或 `[[triggers]]`
    /// 表缺失时返回空 Vec；缺必填字段（`name`/`pattern`/`action`）或正则
    /// 无法解析的条目被跳过（带 `warn`）——配置不可信，绝不能让应用
    /// panic。重复的 `name` 收敛到最后一个定义（后者胜出）。
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
            // 按 name 去重：后定义覆盖先定义。
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
