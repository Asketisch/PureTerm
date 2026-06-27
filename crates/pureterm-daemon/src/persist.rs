//! GAP-5.2: persist live tunnel configs across daemon restarts.
//!
//! `TunnelManager` holds live state (`oneshot::Sender`, `AbortHandle`) that is
//! not serializable, so we snapshot a serializable projection — `TunnelConfig`
//! — into a daemon-owned `tunnels.json`. On startup the file is reloaded and
//! each config is registered as a *stopped* tunnel (config-only restore; the
//! user re-starts explicitly — see the GAP-5.2 plan; auto-reconnect is deferred).
//!
//! Secrets are deliberately NOT persisted (mirrors the SSH-profile pattern): a
//! `TunnelConfig` has no `password` field, so a password-auth tunnel restored on
//! restart re-enters `NeedAuth` rather than hanging on a secret we never stored.
//!
//! The on-disk shape is forward-compatible: every field defaults, so adding
//! fields later is non-breaking (no `version` bump needed). `version` is still
//! recorded for future migrations.

use std::path::Path;

use serde::{Deserialize, Serialize};

fn default_port() -> u16 {
    22
}
fn default_remote_dns() -> bool {
    true
}

/// Serializable projection of a live tunnel. **No `password`/secret field** —
/// see the module docs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelConfig {
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub identity_file: Option<String>,
    pub local_port: u16,
    #[serde(default = "default_remote_dns")]
    pub remote_dns: bool,
    #[serde(default)]
    pub forward_agent: bool,
}

/// On-disk container.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelsFile {
    pub version: u32,
    pub tunnels: Vec<TunnelConfig>,
}

/// Atomically write `cfg` to `path`. Best-effort: logs on failure (a persist
/// error must never crash the daemon).
pub fn save_tunnels(path: &Path, cfg: &TunnelsFile) {
    use std::io::Write;
    let Some(dir) = path.parent() else {
        log::warn!("tunnels: persist path has no parent");
        return;
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("tunnels: create_dir_all failed: {e}");
        return;
    }
    let json = match serde_json::to_string_pretty(cfg) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("tunnels: serialize failed: {e}");
            return;
        }
    };
    let mut tmp = match tempfile::NamedTempFile::new_in(dir) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("tunnels: temp file failed: {e}");
            return;
        }
    };
    if let Err(e) = tmp.write_all(json.as_bytes()) {
        log::warn!("tunnels: write failed: {e}");
        return;
    }
    if let Err(e) = tmp.persist(path) {
        log::warn!("tunnels: persist failed: {e}");
    }
}

/// Load `tunnels.json`. Returns `None` if the file is missing or corrupt.
pub fn load_tunnels(path: &Path) -> Option<TunnelsFile> {
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TunnelsFile {
        TunnelsFile {
            version: 1,
            tunnels: vec![TunnelConfig {
                host: "gw.example".into(),
                user: Some("alice".into()),
                port: 2222,
                identity_file: Some("/home/alice/.ssh/id_ed25519".into()),
                local_port: 1080,
                remote_dns: false,
                forward_agent: true,
            }],
        }
    }

    #[test]
    fn round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tunnels.json");
        save_tunnels(&path, &sample());
        let loaded = load_tunnels(&path).expect("load after save");
        assert_eq!(loaded, sample());
    }

    #[test]
    fn default_port_and_remote_dns_when_absent() {
        // No `port` / `remote_dns` in the JSON → defaults 22 / true.
        let json = r#"{"host":"gw","local_port":1080}"#;
        let cfg: TunnelConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.port, 22);
        assert!(cfg.remote_dns);
        assert_eq!(cfg.host, "gw");
    }

    #[test]
    fn load_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_tunnels(&tmp.path().join("nope.json")).is_none());
    }

    #[test]
    fn load_corrupt_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tunnels.json");
        std::fs::write(&path, b"{ this is not json").unwrap();
        assert!(load_tunnels(&path).is_none());
    }

    #[test]
    fn secret_is_not_persisted() {
        // No password field exists on TunnelConfig, so a serialized config must
        // never contain "password" — guards against a future field being added.
        let json = serde_json::to_string(&sample().tunnels[0]).unwrap();
        assert!(
            !json.contains("password"),
            "TunnelConfig must not serialize any secret: {json}"
        );
    }

    #[test]
    fn version_field_present_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tunnels.json");
        save_tunnels(&path, &sample());
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("\"version\""), "missing version: {on_disk}");
    }
}
