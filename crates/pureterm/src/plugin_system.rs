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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
}
