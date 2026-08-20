//! GAP-7.10 (Phase C): install PureTerm MCP 服务器到 agent 客户端的
//! 配置中——`通过 pureterm mcp install claude-code|codex|claude-desktop` 安装。
//!
//! 每个适配器写入一个启动 `pureterm mcp serve` 的服务器条目。该条目
//! 位于我们独占的命名空间键下(`mcpServers.pureterm`,对应 JSON
//! 客户端;`[mcp_servers.pureterm]` 对应 TOML),因此安装是**幂等**的
//! (先删后插);卸载只动我们的条目,保留用户配置的
//! 其他任何服务器。
//!
//! JSON 客户端(Claude Code 的 `.mcp.json`、Claude Desktop)复用 `agent_adapters`
//! 中的加锁 JSON RMW 引擎;TOML 客户端(Codex)则有并行的
//! `locked_toml_rmw` 实现。

use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde_json::{json, Map, Value};
use toml_edit::{DocumentMut, Item};

use crate::agent_adapters::{locked_json_rmw, AdapterError};

/// 我们在每个客户端配置中独占的服务器键。其他 PureTerm 注册的
/// 服务器可能冲突,但 `pureterm` 是我们的产品名——该命名空间
/// 按约定归我们所有。
const SERVER_KEY: &str = "pureterm";

/// 所有客户端启动我们时使用的参数:`<bin> mcp serve`。
fn serve_args() -> Vec<String> {
    vec!["mcp".to_string(), "serve".to_string()]
}

// ── JSON 应用(Claude Code `.mcp.json` + Claude Desktop) ──────────────────
// 结构(MCP 对两类客户端的标准):
// 结构示例: { "mcpServers": { "pureterm": { "command": "<bin>", "args": ["mcp","serve"] } } }

/// 在 `mcpServers.pureterm` 下插入/刷新我们的服务器。幂等:先移除
/// 之前的 `pureterm` 条目,重复安装不会产生副本。
pub fn apply_mcp_json_install(root: &mut Value, bin: &Path) {
    let Some(obj) = root.as_object_mut() else {
        return;
    };
    if !obj.contains_key("mcpServers") {
        obj.insert("mcpServers".to_string(), Value::Object(Map::new()));
    }
    let servers = obj
        .get_mut("mcpServers")
        .and_then(|v| v.as_object_mut())
        .expect("just inserted mcpServers as object");
    // 先删后插 = 幂等刷新(同时丢弃过期的二进制路径等)。
    servers.remove(SERVER_KEY);
    servers.insert(
        SERVER_KEY.to_string(),
        json!({
            "command": bin.to_string_lossy(),
            "args": serve_args(),
        }),
    );
}

/// 移除我们的服务器;若 `mcpServers` 因此为空,则一并删除,避免
/// 在配置中残留空表。其他服务器不受影响。
pub fn apply_mcp_json_uninstall(root: &mut Value) {
    let Some(servers) = root
        .as_object_mut()
        .and_then(|o| o.get_mut("mcpServers"))
        .and_then(|v| v.as_object_mut())
    else {
        return;
    };
    servers.remove(SERVER_KEY);
    if servers.is_empty() {
        if let Some(obj) = root.as_object_mut() {
            obj.remove("mcpServers");
        }
    }
}

/// 我们的服务器键是否存在(与记录的二进制路径无关——
/// 我们独占该键,任何 `pureterm` 条目都属于我们)。
pub fn has_mcp_json(root: &Value) -> bool {
    root.get("mcpServers")
        .and_then(|s| s.get(SERVER_KEY))
        .is_some()
}

// ── TOML 应用(Codex `~/.codex/config.toml`) ─────────────────────────────
// 结构: [mcp_servers.pureterm],含 command + args。

/// 插入/刷新 `[mcp_servers.pureterm]`。幂等:先移除旧表,
/// 其他 `[mcp_servers.*]` 表(以及文档其余部分)保持不变。
pub fn apply_codex_toml_install(doc: &mut DocumentMut, bin: &Path) {
    // 确保 `mcp_servers` 表存在。
    if !doc.contains_table("mcp_servers") {
        doc.insert("mcp_servers", Item::Table(toml_edit::Table::new()));
    }
    let mcp_servers = doc
        .get_mut("mcp_servers")
        .and_then(|t| t.as_table_mut())
        .expect("just inserted mcp_servers");
    mcp_servers.remove(SERVER_KEY);
    let mut tbl = toml_edit::Table::new();
    tbl.insert("command", toml_edit::value(bin.to_string_lossy().to_string()));
    let arr = toml_edit::Array::from_iter(serve_args().into_iter());
    tbl.insert("args", toml_edit::value(arr));
    // `[mcp_servers.pureterm]` 的装饰仅作显式展示;`decor_mut` 保持默认。
    mcp_servers.insert(SERVER_KEY, Item::Table(tbl));
}

/// 移除 `[mcp_servers.pureterm]`;保留 `[mcp_servers]`(其他服务器可能
/// 仍存在)——toml_edit 即使空表也会保留父表,这没问题。
pub fn apply_codex_toml_uninstall(doc: &mut DocumentMut) {
    if let Some(mcp_servers) = doc.get_mut("mcp_servers").and_then(|t| t.as_table_mut()) {
        mcp_servers.remove(SERVER_KEY);
    }
}

/// `[mcp_servers.pureterm]` 是否存在。
pub fn has_codex_toml(doc: &DocumentMut) -> bool {
    doc.get("mcp_servers")
        .and_then(|t| t.as_table())
        .and_then(|t| t.get(SERVER_KEY))
        .is_some()
}

// ── 加锁 TOML RMW(与 agent_adapters::locked_json_rmw 类似) ──────────

/// 对 TOML 文件进行带建议锁的读-改-写,与
/// `locked_json_rmw` 类似:fs2 排他锁 + 原子的 `NamedTempFile::persist`。
/// 缺失/空文件视为全新的空文档。语法非法的 TOML 文件会报错
/// (绝不会静默破坏用户的配置)。
fn locked_toml_rmw<F>(path: &Path, mutate: F) -> Result<(), AdapterError>
where
    F: FnOnce(&mut DocumentMut) -> Result<(), AdapterError>,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    lock_file.lock_exclusive()?;

    let res = (|| {
        let bytes = std::fs::read(path).unwrap_or_default();
        let mut doc: DocumentMut = if bytes.iter().all(|&b| b.is_ascii_whitespace()) {
            DocumentMut::new()
        } else {
            std::str::from_utf8(&bytes)
                .map_err(|e| {
                    AdapterError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    ))
                })?
                .parse::<DocumentMut>()
                .map_err(|e| {
                    AdapterError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("{}: {e}", path.display()),
                    ))
                })?
        };
        mutate(&mut doc)?;
        let pretty = doc.to_string();
        let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(tmp.path(), pretty.as_bytes())?;
        tmp.persist(path)?;
        Ok(())
    })();

    drop(lock_file);
    res
}

// ── 适配器 trait ─────────────────────────────────────────────────────────

pub trait McpClientAdapter {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn config_path(&self) -> Result<PathBuf, AdapterError>;
    fn install(&self, bin: &Path) -> Result<PathBuf, AdapterError>;
    fn uninstall(&self, bin: &Path) -> Result<PathBuf, AdapterError>;
    fn is_installed(&self, bin: &Path) -> bool;
}

// ── Claude Code:项目级 `.mcp.json`(当前目录) ─────────────────────────
// Claude Code 在项目根目录读取 `.mcp.json`,用于项目级 MCP
// 服务器——非常适合'在 PureTerm 项目内工作的 agent'。

pub struct ClaudeCodeAdapter {
    path_override: Option<PathBuf>,
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        Self { path_override: None }
    }
    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path_override: Some(path) }
    }
}
impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl McpClientAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        "claude-code"
    }
    fn display_name(&self) -> &'static str {
        "Claude Code (.mcp.json)"
    }
    fn config_path(&self) -> Result<PathBuf, AdapterError> {
        if let Some(p) = &self.path_override {
            return Ok(p.clone());
        }
        // 项目级:当前工作目录下的 `.mcp.json`。
        Ok(std::env::current_dir()?.join(".mcp.json"))
    }
    fn install(&self, bin: &Path) -> Result<PathBuf, AdapterError> {
        let path = self.config_path()?;
        locked_json_rmw(&path, |root| {
            apply_mcp_json_install(root, bin);
            Ok(())
        })?;
        Ok(path)
    }
    fn uninstall(&self, _bin: &Path) -> Result<PathBuf, AdapterError> {
        let path = self.config_path()?;
        locked_json_rmw(&path, |root| {
            apply_mcp_json_uninstall(root);
            Ok(())
        })?;
        Ok(path)
    }
    fn is_installed(&self, _bin: &Path) -> bool {
        let Ok(path) = self.config_path() else {
            return false;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        let Ok(root): serde_json::Result<Value> = serde_json::from_slice(&bytes) else {
            return false;
        };
        has_mcp_json(&root)
    }
}

// ── Claude Desktop:macOS 应用支持配置 ──────────────────────────────

pub struct ClaudeDesktopAdapter {
    path_override: Option<PathBuf>,
}

impl ClaudeDesktopAdapter {
    pub fn new() -> Self {
        Self { path_override: None }
    }
    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path_override: Some(path) }
    }
}
impl Default for ClaudeDesktopAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl McpClientAdapter for ClaudeDesktopAdapter {
    fn id(&self) -> &'static str {
        "claude-desktop"
    }
    fn display_name(&self) -> &'static str {
        "Claude Desktop"
    }
    fn config_path(&self) -> Result<PathBuf, AdapterError> {
        if let Some(p) = &self.path_override {
            return Ok(p.clone());
        }
        let home = dirs::home_dir().ok_or(AdapterError::NoHome)?;
        // macOS 路径;在其他系统上该文件创建前不存在。
        Ok(home
            .join("Library/Application Support/Claude/claude_desktop_config.json"))
    }
    fn install(&self, bin: &Path) -> Result<PathBuf, AdapterError> {
        let path = self.config_path()?;
        locked_json_rmw(&path, |root| {
            apply_mcp_json_install(root, bin);
            Ok(())
        })?;
        Ok(path)
    }
    fn uninstall(&self, _bin: &Path) -> Result<PathBuf, AdapterError> {
        let path = self.config_path()?;
        locked_json_rmw(&path, |root| {
            apply_mcp_json_uninstall(root);
            Ok(())
        })?;
        Ok(path)
    }
    fn is_installed(&self, _bin: &Path) -> bool {
        let Ok(path) = self.config_path() else {
            return false;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        let Ok(root): serde_json::Result<Value> = serde_json::from_slice(&bytes) else {
            return false;
        };
        has_mcp_json(&root)
    }
}

// ── Codex 客户端:`~/.codex/config.toml`(TOML) ────────────────────────────────────

pub struct CodexAdapter {
    path_override: Option<PathBuf>,
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self { path_override: None }
    }
    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path_override: Some(path) }
    }
}
impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl McpClientAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }
    fn display_name(&self) -> &'static str {
        "Codex (~/.codex/config.toml)"
    }
    fn config_path(&self) -> Result<PathBuf, AdapterError> {
        if let Some(p) = &self.path_override {
            return Ok(p.clone());
        }
        let home = dirs::home_dir().ok_or(AdapterError::NoHome)?;
        Ok(home.join(".codex").join("config.toml"))
    }
    fn install(&self, bin: &Path) -> Result<PathBuf, AdapterError> {
        let path = self.config_path()?;
        locked_toml_rmw(&path, |doc| {
            apply_codex_toml_install(doc, bin);
            Ok(())
        })?;
        Ok(path)
    }
    fn uninstall(&self, _bin: &Path) -> Result<PathBuf, AdapterError> {
        let path = self.config_path()?;
        locked_toml_rmw(&path, |doc| {
            apply_codex_toml_uninstall(doc);
            Ok(())
        })?;
        Ok(path)
    }
    fn is_installed(&self, _bin: &Path) -> bool {
        let Ok(path) = self.config_path() else {
            return false;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return false;
        };
        let Ok(doc) = text.parse::<DocumentMut>() else {
            return false;
        };
        has_codex_toml(&doc)
    }
}

// ── 注册表 + 分发 ───────────────────────────────────────────────────

pub fn registry() -> Vec<Box<dyn McpClientAdapter>> {
    vec![
        Box::new(ClaudeCodeAdapter::new()),
        Box::new(CodexAdapter::new()),
        Box::new(ClaudeDesktopAdapter::new()),
    ]
}

pub fn find_adapter(id: &str) -> Option<Box<dyn McpClientAdapter>> {
    registry().into_iter().find(|a| a.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn bin() -> PathBuf {
        PathBuf::from("/opt/pureterm/bin/pureterm")
    }

    fn claude_code_adapter_at(path: PathBuf) -> ClaudeCodeAdapter {
        ClaudeCodeAdapter::at(path)
    }

    fn codex_adapter_at(path: PathBuf) -> CodexAdapter {
        CodexAdapter::at(path)
    }

    // ── 纯 JSON 应用函数(Claude Code 与 Claude Desktop 共用) ──

    #[test]
    fn json_install_adds_pureterm_server() {
        let mut root = json!({});
        apply_mcp_json_install(&mut root, &bin());
        let ours = &root["mcpServers"]["pureterm"];
        assert_eq!(ours["command"], "/opt/pureterm/bin/pureterm");
        assert_eq!(ours["args"], json!(["mcp", "serve"]));
    }

    #[test]
    fn json_install_into_existing_mcpservers() {
        let mut root = json!({ "mcpServers": { "filesystem": { "command": "npx" } } });
        apply_mcp_json_install(&mut root, &bin());
        // 外部服务器保留。
        assert_eq!(root["mcpServers"]["filesystem"]["command"], "npx");
        // 我们的服务器已添加。
        assert_eq!(root["mcpServers"]["pureterm"]["args"], json!(["mcp", "serve"]));
    }

    #[test]
    fn json_install_is_idempotent() {
        let mut root = json!({});
        apply_mcp_json_install(&mut root, &bin());
        apply_mcp_json_install(&mut root, &bin()); // 再次运行 = 刷新而非重复
        let servers = root["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 1, "pureterm should be the only server");
    }

    #[test]
    fn json_uninstall_removes_only_ours() {
        let mut root = json!({
            "mcpServers": {
                "filesystem": { "command": "npx" },
                "pureterm": { "command": "pureterm", "args": ["mcp", "serve"] }
            }
        });
        apply_mcp_json_uninstall(&mut root);
        assert!(root["mcpServers"].get("pureterm").is_none());
        assert_eq!(root["mcpServers"]["filesystem"]["command"], "npx");
    }

    // ── 纯 TOML 应用函数(Codex) ──

    #[test]
    fn toml_install_adds_pureterm_table() {
        let mut doc = toml_edit::DocumentMut::new();
        apply_codex_toml_install(&mut doc, &bin());
        let tbl = doc
            .get("mcp_servers")
            .and_then(|t| t.as_table())
            .and_then(|t| t.get("pureterm"))
            .and_then(|t| t.as_table())
            .expect("mcp_servers.pureterm table");
        assert_eq!(tbl.get("command").unwrap().as_str(), Some("/opt/pureterm/bin/pureterm"));
        let args = tbl.get("args").unwrap().as_array().unwrap();
        let vals: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(vals, vec!["mcp", "serve"]);
    }

    #[test]
    fn toml_install_is_idempotent() {
        let mut doc = toml_edit::DocumentMut::new();
        apply_codex_toml_install(&mut doc, &bin());
        apply_codex_toml_install(&mut doc, &bin());
        let count = doc
            .get("mcp_servers")
            .and_then(|t| t.as_table())
            .map(|t| t.len())
            .unwrap_or(0);
        assert_eq!(count, 1);
    }

    #[test]
    fn toml_install_preserves_foreign_servers() {
        let mut doc: toml_edit::DocumentMut =
            "[mcp_servers.filesystem]\ncommand = \"npx\"\n".parse().unwrap();
        apply_codex_toml_install(&mut doc, &bin());
        assert_eq!(
            doc.get("mcp_servers")
                .and_then(|t| t.as_table())
                .and_then(|t| t.get("filesystem"))
                .and_then(|t| t.as_table())
                .and_then(|t| t.get("command"))
                .and_then(|v| v.as_str()),
            Some("npx")
        );
    }

    #[test]
    fn toml_uninstall_removes_only_ours() {
        let mut doc: toml_edit::DocumentMut =
            "[mcp_servers.filesystem]\ncommand = \"npx\"\n[mcp_servers.pureterm]\ncommand = \"pureterm\"\n"
                .parse()
                .unwrap();
        apply_codex_toml_uninstall(&mut doc);
        assert!(
            doc.get("mcp_servers")
                .and_then(|t| t.as_table())
                .and_then(|t| t.get("pureterm"))
                .is_none()
        );
        assert_eq!(
            doc.get("mcp_servers")
                .and_then(|t| t.as_table())
                .and_then(|t| t.get("filesystem"))
                .and_then(|t| t.as_table())
                .and_then(|t| t.get("command"))
                .and_then(|v| v.as_str()),
            Some("npx")
        );
    }

    // ── 通过适配器的端到端测试(基于文件、临时目录) ──

    #[test]
    fn claude_code_install_roundtrip_via_adapter() {
        let dir = tempdir().unwrap();
        let adp = claude_code_adapter_at(dir.path().join(".mcp.json"));
        let path = adp.install(&bin()).unwrap();
        assert!(adp.is_installed(&bin()), "is_installed after install");
        // 原始文件包含我们的服务器。
        let bytes = std::fs::read(&path).unwrap();
        let root: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(root["mcpServers"]["pureterm"]["args"], json!(["mcp", "serve"]));

        adp.uninstall(&bin()).unwrap();
        assert!(!adp.is_installed(&bin()), "not installed after uninstall");
    }

    #[test]
    fn codex_install_roundtrip_via_adapter() {
        let dir = tempdir().unwrap();
        let adp = codex_adapter_at(dir.path().join("config.toml"));
        adp.install(&bin()).unwrap();
        assert!(adp.is_installed(&bin()));
        let raw = std::fs::read_to_string(adp.config_path().unwrap()).unwrap();
        assert!(raw.contains("[mcp_servers.pureterm]"), "raw: {raw}");

        adp.uninstall(&bin()).unwrap();
        assert!(!adp.is_installed(&bin()));
    }

    #[test]
    fn install_preserves_a_foreign_server_on_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".mcp.json");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "mcpServers": { "filesystem": { "command": "npx" } }
            }))
            .unwrap(),
        )
        .unwrap();

        let adp = claude_code_adapter_at(path);
        adp.install(&bin()).unwrap();
        let after: serde_json::Value =
            serde_json::from_slice(&std::fs::read(adp.config_path().unwrap()).unwrap()).unwrap();
        assert_eq!(after["mcpServers"]["filesystem"]["command"], "npx");
        assert_eq!(after["mcpServers"]["pureterm"]["args"], json!(["mcp", "serve"]));
    }
}
