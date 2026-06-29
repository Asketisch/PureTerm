//! GAP-44: Shared sessions — multi-user terminal (XL stub: config + serve scaffold).

use serde::{Deserialize, Serialize};

/// `[shared_session]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedSessionConfig {
    pub enabled: bool,
    pub port: u16,
    pub read_only_default: bool,
}

impl Default for SharedSessionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8765,
            read_only_default: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShareInvite {
    pub url: String,
    pub token: String,
    pub read_only: bool,
}

/// Generate a share invite URL (WebSocket server not started in MVP stub).
pub fn create_invite(cfg: &SharedSessionConfig, host: &str) -> Result<ShareInvite, String> {
    if !cfg.enabled {
        return Err("shared_session disabled — set [shared_session].enabled = true".into());
    }
    let token = format!("pt-{:x}", simple_hash(host));
    Ok(ShareInvite {
        url: format!("pureterm://share?host={host}&port={}&token={token}", cfg.port),
        token,
        read_only: cfg.read_only_default,
    })
}

/// Stub server entry — logs and returns immediately.
pub fn serve_stub(cfg: &SharedSessionConfig) -> Result<(), String> {
    if !cfg.enabled {
        return Err("shared_session disabled".into());
    }
    eprintln!(
        "GAP-44 stub: would listen on 0.0.0.0:{} (WebRTC/WebSocket deferred)",
        cfg.port
    );
    Ok(())
}

fn simple_hash(s: &str) -> u64 {
    s.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_when_enabled() {
        let cfg = SharedSessionConfig {
            enabled: true,
            ..Default::default()
        };
        let inv = create_invite(&cfg, "127.0.0.1").unwrap();
        assert!(inv.url.contains("pureterm://share"));
    }
}
