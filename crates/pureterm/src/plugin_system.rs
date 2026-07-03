//! GAP-49: Plugin system — directory scan + manifest parsing + install.
//!
//! ## Model
//!
//! A "plugin" is a folder under [`PluginSystemConfig::plugin_dir`]
//! (default `~/.config/pureterm/plugins/<name>/`) that contains a
//! `plugin.toml` manifest. The manifest is a tiny TOML file with:
//!
//! ```toml
//! name = "my-plugin"      # required, must match the folder name
//! version = "0.1.0"       # required, semver-ish
//! description = ""        # optional
//! author = ""             # optional
//! # Future extension points (not yet wired into the runtime):
//! # provides = ["profile_provider", "status_bar_component"]
//! ```
//!
//! The runtime (WASM / dylib) is intentionally deferred. The scaffold
//! only validates manifests, lists plugins, and installs/updates stubs.
//! Hooking plugins into the terminal surface is a Phase D concern.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `[plugins]` config.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSystemConfig {
    pub enabled: bool,
    pub directory: Option<PathBuf>,
}

impl Default for PluginSystemConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            directory: None,
        }
    }
}

impl PluginSystemConfig {
    pub fn plugin_dir(&self) -> PathBuf {
        self.directory.clone().unwrap_or_else(default_plugin_dir)
    }
}

fn default_plugin_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pureterm")
        .join("plugins")
}

/// On-disk manifest (`<plugin>/plugin.toml`).
///
/// t-core-plugin-runtime extends the v0 manifest (GAP-49) with the
/// `abi_version` / `entry_points` / `provides` / `config_schema`
/// fields. v0 manifests (without these fields) keep working: every
/// new field is `#[serde(default)]` and the loader fills in
/// sensible defaults. The v1 ABI version is `1`; future bumps
/// require a `[pureterm abi = "N"]` opt-in from the loader.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    /// ABI version this plugin was compiled against. The loader
    /// refuses to load a plugin whose `abi_version` is greater than
    /// the running binary's max supported (currently 1).
    #[serde(default = "default_abi_version")]
    pub abi_version: u32,
    /// Entry points: a name → function-index map inside the WASM
    /// module. v1 keys are the strings in [`PLUGIN_ENTRY_POINTS`].
    /// The map is empty for v0 manifests.
    #[serde(default)]
    pub entry_points: std::collections::HashMap<String, String>,
    /// Capability declarations. v1 stores them as strings; the
    /// loader parses each via [`Provides::from_str`].
    #[serde(default)]
    pub provides: Vec<String>,
    /// Optional JSON-schema-ish config block. v1: just a free-form
    /// TOML table the GUI can pass to the plugin's `on_config`
    /// host call.
    #[serde(default)]
    pub config_schema: Option<toml::Value>,
    /// T-core-plugin-runtime v2: per-plugin enable / disable. Flipped
    /// by the `pureterm plugin enable|disable <name>` CLI; absent
    /// defaults to `false` (the plugin loader treats the absence as
    /// "user hasn't enabled it yet").
    #[serde(default)]
    pub enabled: bool,
}

fn default_abi_version() -> u32 {
    1
}

/// Recognized `provides = [...]` entries. The strings serialize via
/// `as_str`; the loader's parser is the inverse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provides {
    /// A profile (SSH / shell profile) provider.
    ProfileProvider,
    /// A host-app contribution (menu item, deep link, etc.).
    HostApp,
    /// A toolbar button in the GUI chrome.
    ToolbarButtonProvider,
    /// A context-menu item in the tab bar.
    TabContextMenuProvider,
    /// A config-file provider (e.g. for a remote config backend).
    ConfigProvider,
    /// A status-bar component (a widget rendered in the status bar).
    StatusBarComponent,
    /// A sidebar tab (a custom page in the right sidebar).
    SidebarTab,
    /// A command-palette item (a fuzzy-matchable action).
    CommandPaletteItem,
    /// An IPC method (custom JSON-RPC endpoint).
    IpcMethod,
    /// A terminal-parser hook (custom OSC/CSI/DCS sequences).
    TerminalParserHook,
    /// A renderer overlay (custom draw call on top of the terminal
    /// grid).
    RendererOverlay,
}

impl Provides {
    /// Canonical string form (used in `plugin.toml` and JSON
    /// serialization).
    pub fn as_str(&self) -> &'static str {
        match self {
            Provides::ProfileProvider => "profile_provider",
            Provides::HostApp => "host_app",
            Provides::ToolbarButtonProvider => "toolbar_button_provider",
            Provides::TabContextMenuProvider => "tab_context_menu_provider",
            Provides::ConfigProvider => "config_provider",
            Provides::StatusBarComponent => "status_bar_component",
            Provides::SidebarTab => "sidebar_tab",
            Provides::CommandPaletteItem => "command_palette_item",
            Provides::IpcMethod => "ipc_method",
            Provides::TerminalParserHook => "terminal_parser_hook",
            Provides::RendererOverlay => "renderer_overlay",
        }
    }

    /// Parse from the canonical string. Returns `None` for unknown
    /// values (forward-compat: a v0 loader skips unknown provides
    /// with a warning rather than aborting).
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "profile_provider" => Some(Provides::ProfileProvider),
            "host_app" => Some(Provides::HostApp),
            "toolbar_button_provider" => Some(Provides::ToolbarButtonProvider),
            "tab_context_menu_provider" => Some(Provides::TabContextMenuProvider),
            "config_provider" => Some(Provides::ConfigProvider),
            "status_bar_component" => Some(Provides::StatusBarComponent),
            "sidebar_tab" => Some(Provides::SidebarTab),
            "command_palette_item" => Some(Provides::CommandPaletteItem),
            "ipc_method" => Some(Provides::IpcMethod),
            "terminal_parser_hook" => Some(Provides::TerminalParserHook),
            "renderer_overlay" => Some(Provides::RendererOverlay),
            _ => None,
        }
    }
}

/// The full set of entry-point names a plugin can declare. v0
/// plugins (no `entry_points` map) get an empty map and are loaded
/// as inert stubs (the v0 surface — `install_stub` / `list_plugins`
/// — only validates the manifest, never invokes the plugin).
pub const PLUGIN_ENTRY_POINTS: &[&str] = &[
    "on_load",
    "on_unload",
    "on_config",
    "on_palette_command",
    "on_ipc_method",
    "on_status_bar_tick",
    "on_parser_osc",
    "on_render_overlay",
];

#[derive(Clone, Debug, PartialEq)]
pub struct PluginEntry {
    pub name: String,
    pub path: PathBuf,
    pub manifest: Option<PluginManifest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginError {
    pub plugin: String,
    pub reason: String,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.plugin, self.reason)
    }
}

/// Validate a parsed manifest against its folder name. Centralized so
/// `load_manifest` and `install_stub` share the same rules.
pub(crate) fn validate_manifest(folder_name: &str, m: &PluginManifest) -> Result<(), String> {
    if m.name != folder_name {
        return Err(format!(
            "manifest name \"{}\" does not match folder \"{}\"",
            m.name, folder_name
        ));
    }
    if m.name.is_empty() || m.name.contains('/') {
        return Err("plugin name must be non-empty and not contain '/'".into());
    }
    if m.version.is_empty() {
        return Err("version is required".into());
    }
    Ok(())
}

/// Load and validate a manifest from a plugin directory. Returns the
/// parsed manifest on success, or an error describing what went wrong.
pub fn load_manifest(plugin_dir: &Path) -> Result<PluginManifest, String> {
    let folder_name = plugin_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "plugin path has no folder name".to_string())?;
    let manifest_path = plugin_dir.join("plugin.toml");
    if !manifest_path.exists() {
        return Err(format!(
            "missing plugin.toml in {}",
            plugin_dir.display()
        ));
    }
    let raw = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let m: PluginManifest = toml::from_str(&raw).map_err(|e| e.to_string())?;
    validate_manifest(folder_name, &m)?;
    Ok(m)
}

/// List plugin folders (one subdir per plugin) and parse their manifests.
/// Manifest errors do not abort the listing — instead the plugin is
/// returned with `manifest = None` and a [`PluginError`] is collected.
pub fn list_plugins(cfg: &PluginSystemConfig) -> Vec<PluginEntry> {
    list_plugins_with_errors(cfg).0
}

/// Same as [`list_plugins`] but also returns per-plugin manifest errors.
/// Useful for the CLI / settings UI which want to surface "bad manifest"
/// in addition to the valid entries.
pub fn list_plugins_with_errors(
    cfg: &PluginSystemConfig,
) -> (Vec<PluginEntry>, Vec<PluginError>) {
    if !cfg.enabled {
        return (Vec::new(), Vec::new());
    }
    let root = cfg.plugin_dir();
    let Ok(read) = std::fs::read_dir(&root) else {
        return (Vec::new(), Vec::new());
    };
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for entry in read.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        match load_manifest(&path) {
            Ok(manifest) => entries.push(PluginEntry {
                name,
                path,
                manifest: Some(manifest),
            }),
            Err(reason) => errors.push(PluginError { plugin: name, reason }),
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    (entries, errors)
}

/// Install a stub plugin: create the directory and write a minimal
/// `plugin.toml` manifest. If the manifest already exists and parses
/// successfully, the call is a no-op (idempotent).
pub fn install_stub(cfg: &PluginSystemConfig, name: &str) -> Result<PathBuf, String> {
    if !cfg.enabled {
        return Err("plugins disabled".into());
    }
    if name.is_empty() || name.contains('/') {
        return Err("invalid plugin name".into());
    }
    let dir = cfg.plugin_dir().join(name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let manifest = dir.join("plugin.toml");
    if !manifest.exists() {
        let body = format!(
            "# GAP-49 plugin stub\nname = \"{name}\"\nversion = \"0.1.0\"\ndescription = \"\"\nauthor = \"\"\n"
        );
        std::fs::write(&manifest, body).map_err(|e| e.to_string())?;
    } else {
        // Verify the existing manifest is still valid (catches "someone
        // edited plugin.toml and broke the name match").
        load_manifest(&dir)?;
    }
    Ok(dir)
}

/// Uninstall a plugin by removing its directory. Refuses to run when
/// the manifest cannot be located (defensive — we don't want to
/// `rm -rf` an arbitrary folder).
pub fn uninstall(cfg: &PluginSystemConfig, name: &str) -> Result<(), String> {
    if !cfg.enabled {
        return Err("plugins disabled".into());
    }
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err("invalid plugin name".into());
    }
    let dir = cfg.plugin_dir().join(name);
    // The directory must look like a plugin dir: contain a parseable
    // plugin.toml with the matching name. This guards against typos
    // and shell-glob accidents.
    load_manifest(&dir)?;
    std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())
}

/// T-core-plugin-runtime v2: flip the `enabled` flag on a plugin's
/// manifest in-place. Returns Err if the plugin isn't installed or
/// plugins are globally disabled.
///
/// The flag is read by the GUI on next startup; v3 will live-reload
/// the manifest without requiring a restart.
pub fn set_enabled(cfg: &PluginSystemConfig, name: &str, enabled: bool) -> Result<(), String> {
    if !cfg.enabled {
        return Err("plugins disabled".into());
    }
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err("invalid plugin name".into());
    }
    let manifest_path = cfg.plugin_dir().join(name).join("plugin.toml");
    if !manifest_path.exists() {
        return Err(format!("plugin not installed: {name}"));
    }
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("parse {}: {e}", manifest_path.display()))?;
    // Set `enabled = true|false` at the document root. Plugins that
    // don't have an `[enabled]` flag yet get it created.
    doc["enabled"] = toml_edit::value(enabled);
    std::fs::write(&manifest_path, doc.to_string())
        .map_err(|e| format!("write {}: {e}", manifest_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_cfg(dir: &Path) -> PluginSystemConfig {
        PluginSystemConfig {
            enabled: true,
            directory: Some(dir.to_path_buf()),
        }
    }

    #[test]
    fn list_empty_when_disabled() {
        assert!(list_plugins(&PluginSystemConfig::default()).is_empty());
    }

    #[test]
    fn list_empty_when_dir_missing() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_does_not_exist_xyz");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        assert!(list_plugins(&cfg).is_empty());
    }

    #[test]
    fn install_creates_manifest_that_loads() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_install_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        let dir = install_stub(&cfg, "hello").unwrap();
        let m = load_manifest(&dir).unwrap();
        assert_eq!(m.name, "hello");
        assert_eq!(m.version, "0.1.0");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_rejects_empty_name() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_install_invalid");
        let cfg = enabled_cfg(&tmp);
        assert!(install_stub(&cfg, "").is_err());
        assert!(install_stub(&cfg, "a/b").is_err());
    }

    /// T-core-plugin-runtime v2: `enable` / `disable` flip the
    /// `enabled` flag on the plugin's `plugin.toml`. After flipping,
    /// re-reading the manifest reports the new state.
    #[test]
    fn set_enabled_flips_manifest_flag() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_set_enabled_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        let dir = install_stub(&cfg, "toggle").unwrap();
        // Default state: enabled flag absent (defaults to false on read).
        let m_before = load_manifest(&dir).unwrap();
        assert_ne!(m_before.enabled, true, "stub installs with no enabled flag");

        // Flip to true.
        set_enabled(&cfg, "toggle", true).expect("enable");
        let m_on = load_manifest(&dir).unwrap();
        assert!(m_on.enabled, "after enable, flag must be true");

        // Flip back to false.
        set_enabled(&cfg, "toggle", false).expect("disable");
        let m_off = load_manifest(&dir).unwrap();
        assert!(!m_off.enabled, "after disable, flag must be false");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Setting enabled on a missing plugin surfaces an Err.
    #[test]
    fn set_enabled_rejects_unknown_plugin() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_set_enabled_unknown");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        let err = set_enabled(&cfg, "ghost", true).unwrap_err();
        assert!(err.contains("not installed"), "must report not installed: {err}");
    }

    /// `set_enabled` rejects empty / path-traversal names just like
    /// the rest of the plugin API.
    #[test]
    fn set_enabled_rejects_invalid_names() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_set_enabled_invalid");
        let cfg = enabled_cfg(&tmp);
        assert!(set_enabled(&cfg, "", true).is_err());
        assert!(set_enabled(&cfg, "a/b", true).is_err());
        assert!(set_enabled(&cfg, "..", true).is_err());
    }

    /// Plugins disabled globally block the per-plugin toggle too.
    #[test]
    fn set_enabled_rejects_when_plugins_globally_disabled() {
        let cfg = PluginSystemConfig::default();
        assert!(set_enabled(&cfg, "any", true).is_err());
    }

    #[test]
    fn install_rejects_when_disabled() {
        let cfg = PluginSystemConfig::default();
        assert!(install_stub(&cfg, "hello").is_err());
    }

    #[test]
    fn list_reports_manifest_errors_separately() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_mixed");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        // Good plugin
        install_stub(&cfg, "good").unwrap();
        // Bad plugin: folder exists, no plugin.toml
        std::fs::create_dir_all(tmp.join("broken")).unwrap();
        let (entries, errors) = list_plugins_with_errors(&cfg);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "good");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].plugin, "broken");
        assert!(errors[0].reason.contains("missing plugin.toml"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn manifest_name_mismatch_is_rejected() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_mismatch");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        let dir = install_stub(&cfg, "alpha").unwrap();
        // Overwrite manifest with a name that doesn't match the folder.
        std::fs::write(
            dir.join("plugin.toml"),
            "name = \"beta\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let res = load_manifest(&dir);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("does not match folder"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uninstall_removes_directory() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_uninstall");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        install_stub(&cfg, "to_remove").unwrap();
        assert!(tmp.join("to_remove").exists());
        uninstall(&cfg, "to_remove").unwrap();
        assert!(!tmp.join("to_remove").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uninstall_refuses_non_plugin_dir() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_uninstall_safe");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        // Folder exists but has no plugin.toml
        std::fs::create_dir_all(tmp.join("not_a_plugin")).unwrap();
        let res = uninstall(&cfg, "not_a_plugin");
        assert!(res.is_err());
        // Folder should still exist
        assert!(tmp.join("not_a_plugin").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- t-core-plugin-runtime: new manifest fields ----

    #[test]
    fn v0_manifest_parses_with_default_abi() {
        let raw = "name = \"old\"\nversion = \"0.1.0\"\n";
        let m: PluginManifest = toml::from_str(raw).expect("parse v0");
        assert_eq!(m.abi_version, 1);
        assert!(m.entry_points.is_empty());
        assert!(m.provides.is_empty());
        assert!(m.config_schema.is_none());
    }

    #[test]
    fn v1_manifest_parses_with_provides_and_entries() {
        let raw = r#"
name = "status-cpu"
version = "0.2.0"
description = "Status-bar CPU monitor"
abi_version = 1
entry_points = { on_load = "load", on_status_bar_tick = "tick" }
provides = ["status_bar_component", "command_palette_item"]
config_schema = { interval_ms = 1000 }
"#;
        let m: PluginManifest = toml::from_str(raw).expect("parse v1");
        assert_eq!(m.abi_version, 1);
        assert_eq!(m.entry_points.get("on_load"), Some(&"load".to_string()));
        assert_eq!(m.entry_points.get("on_status_bar_tick"), Some(&"tick".to_string()));
        assert_eq!(m.provides, vec!["status_bar_component", "command_palette_item"]);
        assert!(m.config_schema.is_some());
    }

    #[test]
    fn provides_str_round_trip() {
        for p in [
            Provides::ProfileProvider,
            Provides::HostApp,
            Provides::ToolbarButtonProvider,
            Provides::TabContextMenuProvider,
            Provides::ConfigProvider,
            Provides::StatusBarComponent,
            Provides::SidebarTab,
            Provides::CommandPaletteItem,
            Provides::IpcMethod,
            Provides::TerminalParserHook,
            Provides::RendererOverlay,
        ] {
            let s = p.as_str();
            let back = Provides::from_str(s).expect("parse known");
            assert_eq!(back, p);
        }
    }

    #[test]
    fn provides_from_str_unknown_is_none() {
        // Forward-compat: an unknown provides value is a no-op (the
        // GUI logs a warning and continues loading the plugin).
        assert!(Provides::from_str("future_thing").is_none());
        assert!(Provides::from_str("").is_none());
    }

    #[test]
    fn plugin_entry_points_contains_v1_set() {
        // Sanity check the documented entry points; the renderer
        // and IPC code paths dispatch on these names.
        let v: Vec<&&str> = PLUGIN_ENTRY_POINTS.iter().collect();
        assert!(v.contains(&&"on_load"));
        assert!(v.contains(&&"on_unload"));
        assert!(v.contains(&&"on_palette_command"));
        assert!(v.contains(&&"on_ipc_method"));
    }
}
