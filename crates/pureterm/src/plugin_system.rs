//! GAP-49: Plugin system — directory scan + install stub (XL: runtime deferred).

use std::path::PathBuf;

/// `[plugins]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginEntry {
    pub name: String,
    pub path: PathBuf,
}

/// List plugin folders (one subdir per plugin).
pub fn list_plugins(cfg: &PluginSystemConfig) -> Vec<PluginEntry> {
    if !cfg.enabled {
        return Vec::new();
    }
    let root = cfg.plugin_dir();
    let Ok(read) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    read.filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| PluginEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            path: e.path(),
        })
        .collect()
}

/// Install stub: create plugin directory with manifest placeholder.
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
            "# GAP-49 plugin stub\nname = \"{name}\"\nversion = \"0.0.0\"\n"
        );
        std::fs::write(&manifest, body).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_empty_when_disabled() {
        assert!(list_plugins(&PluginSystemConfig::default()).is_empty());
    }
}
