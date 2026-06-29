//! GAP-50: Settings cloud sync — local export/import MVP (Gist deferred).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `[cloud_sync]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudSyncConfig {
    pub enabled: bool,
    pub gist_id: String,
    pub token_env: String,
}

impl Default for CloudSyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gist_id: String::new(),
            token_env: "PURETERM_GIST_TOKEN".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SyncBundle {
    pub config_toml: String,
    pub exported_at: String,
}

fn sync_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pureterm")
        .join("sync")
}

/// Push: copy config to local sync bundle (remote Gist deferred).
pub fn sync_push(cfg: &CloudSyncConfig, config_path: &Path) -> Result<PathBuf, String> {
    if !cfg.enabled {
        return Err("cloud_sync disabled".into());
    }
    let body = std::fs::read_to_string(config_path).map_err(|e| e.to_string())?;
    let bundle = SyncBundle {
        config_toml: body,
        exported_at: chrono::Utc::now().to_rfc3339(),
    };
    let dir = sync_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join("latest.json");
    let json = serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())?;
    std::fs::write(&out, json).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Pull: restore config from local sync bundle.
pub fn sync_pull(cfg: &CloudSyncConfig, config_path: &Path) -> Result<(), String> {
    if !cfg.enabled {
        return Err("cloud_sync disabled".into());
    }
    let path = sync_dir().join("latest.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let bundle: SyncBundle = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    std::fs::write(config_path, bundle.config_toml).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_requires_enabled() {
        assert!(sync_push(&CloudSyncConfig::default(), Path::new("/nope")).is_err());
    }
}
