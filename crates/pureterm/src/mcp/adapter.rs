//! GAP-7.10 (Phase C): install PureTerm's MCP server into an agent client's
//! config — `pureterm mcp install claude-code|codex|claude-desktop`.
//!
//! Each adapter writes a server entry that spawns `pureterm mcp serve`. The
//! entry is namespaced under a key we own (`mcpServers.pureterm` for JSON
//! clients, `[mcp_servers.pureterm]` for TOML), so install is **idempotent**
//! (remove-then-reinsert) and uninstall touches only our entry, preserving any
//! other servers the user configured.
//!
//! JSON clients (Claude Code `.mcp.json`, Claude Desktop) reuse the locked
//! JSON RMW engine from `agent_adapters`; the TOML client (Codex) has a
//! parallel `locked_toml_rmw`.

use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde_json::{json, Map, Value};
use toml_edit::{DocumentMut, Item};

use crate::agent_adapters::{locked_json_rmw, AdapterError};

/// The server key we own in every client config. Other PureTerm-registered
/// servers would collide, but `pureterm` is our product name — the namespace
/// is ours by convention.
const SERVER_KEY: &str = "pureterm";

/// Args every client uses to launch us: `<bin> mcp serve`.
fn serve_args() -> Vec<String> {
    vec!["mcp".to_string(), "serve".to_string()]
}

// ── JSON apply (Claude Code `.mcp.json` + Claude Desktop) ──────────────────
// Shape (MCP standard for both clients):
//   { "mcpServers": { "pureterm": { "command": "<bin>", "args": ["mcp","serve"] } } }

/// Insert/refresh our server under `mcpServers.pureterm`. Idempotent: removes
/// any prior `pureterm` entry first, so re-running install never duplicates.
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
    // Remove-then-insert = idempotent refresh (drops a stale bin path, etc.).
    servers.remove(SERVER_KEY);
    servers.insert(
        SERVER_KEY.to_string(),
        json!({
            "command": bin.to_string_lossy(),
            "args": serve_args(),
        }),
    );
}

/// Remove our server; if `mcpServers` is left empty, drop it too so we don't
/// litter the config with an empty table. Other servers are untouched.
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

/// True iff our server key is present (regardless of the recorded bin path —
/// we own the key, so any `pureterm` entry is ours).
pub fn has_mcp_json(root: &Value) -> bool {
    root.get("mcpServers")
        .and_then(|s| s.get(SERVER_KEY))
        .is_some()
}

// ── TOML apply (Codex `~/.codex/config.toml`) ─────────────────────────────
// Shape: [mcp_servers.pureterm] with command + args.

/// Insert/refresh `[mcp_servers.pureterm]`. Idempotent: removes a prior table
/// first. Foreign `[mcp_servers.*]` tables (and the rest of the doc) survive.
pub fn apply_codex_toml_install(doc: &mut DocumentMut, bin: &Path) {
    // Ensure `mcp_servers` table exists.
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
    // `[mcp_servers.pureterm]` is explicitly decorative; `decor_mut` left default.
    mcp_servers.insert(SERVER_KEY, Item::Table(tbl));
}

/// Remove `[mcp_servers.pureterm]`; leave `[mcp_servers]` (other servers may
/// remain) — toml_edit keeps the parent table even if empty, which is fine.
pub fn apply_codex_toml_uninstall(doc: &mut DocumentMut) {
    if let Some(mcp_servers) = doc.get_mut("mcp_servers").and_then(|t| t.as_table_mut()) {
        mcp_servers.remove(SERVER_KEY);
    }
}

/// True iff `[mcp_servers.pureterm]` is present.
pub fn has_codex_toml(doc: &DocumentMut) -> bool {
    doc.get("mcp_servers")
        .and_then(|t| t.as_table())
        .and_then(|t| t.get(SERVER_KEY))
        .is_some()
}

// ── Locked TOML RMW (parallel to agent_adapters::locked_json_rmw) ──────────

/// Advisory-locked read-modify-write over a TOML file, mirroring
/// `locked_json_rmw`: fs2 exclusive lock + atomic `NamedTempFile::persist`.
/// A missing/empty file is treated as a fresh empty document. A syntactically
/// invalid TOML file errors out (never silently clobbers the user's config).
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

// ── Adapter trait ─────────────────────────────────────────────────────────

pub trait McpClientAdapter {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn config_path(&self) -> Result<PathBuf, AdapterError>;
    fn install(&self, bin: &Path) -> Result<PathBuf, AdapterError>;
    fn uninstall(&self, bin: &Path) -> Result<PathBuf, AdapterError>;
    fn is_installed(&self, bin: &Path) -> bool;
}

// ── Claude Code: project-scoped `.mcp.json` (cwd) ─────────────────────────
// Claude Code reads `.mcp.json` at the project root for project-scoped MCP
// servers — the natural fit for "agent working inside this PureTerm project".

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
        // Project-scoped: the `.mcp.json` in the current working directory.
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

// ── Claude Desktop: macOS app-support config ──────────────────────────────

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
        // macOS path; on other OSes this simply won't exist until created.
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

// ── Codex: ~/.codex/config.toml (TOML) ────────────────────────────────────

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

// ── Registry + dispatch ───────────────────────────────────────────────────

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

    // ── pure JSON apply functions (Claude Code + Claude Desktop share these) ──

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
        // Foreign server preserved.
        assert_eq!(root["mcpServers"]["filesystem"]["command"], "npx");
        // Ours added.
        assert_eq!(root["mcpServers"]["pureterm"]["args"], json!(["mcp", "serve"]));
    }

    #[test]
    fn json_install_is_idempotent() {
        let mut root = json!({});
        apply_mcp_json_install(&mut root, &bin());
        apply_mcp_json_install(&mut root, &bin()); // re-run = refresh, not duplicate
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

    // ── pure TOML apply functions (Codex) ──

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

    // ── end-to-end via the adapters (file-based, tempdir) ──

    #[test]
    fn claude_code_install_roundtrip_via_adapter() {
        let dir = tempdir().unwrap();
        let adp = claude_code_adapter_at(dir.path().join(".mcp.json"));
        let path = adp.install(&bin()).unwrap();
        assert!(adp.is_installed(&bin()), "is_installed after install");
        // Raw file has our server.
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
