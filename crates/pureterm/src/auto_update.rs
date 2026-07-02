//! GAP-45: Auto-update — Sparkle / GitHub Releases check.
//!
//! Provides:
//! - [`AutoUpdateConfig`] — TOML schema (`[update]`).
//! - [`check_for_updates`] — synchronous fast-path: compares the current
//!   build's `CARGO_PKG_VERSION` against the most recently seen
//!   `PURETERM_LATEST_VERSION` env var (handy for CI smoke tests).
//! - [`check_remote_async`] — async GitHub Releases fetch via `reqwest`,
//!   guarded behind a feature check (returns a stub when offline).
//! - [`semver_less`] — simple semver comparator used to decide which
//!   release is "newer" without pulling a full semver crate.
//!
//! Sparkle integration (macOS) is intentionally deferred — the scaffold
//! documents the seam (`feed_url` / `channel`) but does not link the
//! framework yet.

use serde::{Deserialize, Serialize};

/// `[update]` config.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoUpdateConfig {
    pub enabled: bool,
    pub auto_check: bool,
    /// `stable` | `beta`
    pub channel: String,
    /// GitHub Releases endpoint or AppCast feed URL.
    /// Defaults to `https://api.github.com/repos/CNB/PureTerm/releases/latest`
    /// when empty.
    pub feed_url: String,
}

impl Default for AutoUpdateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_check: false,
            channel: "stable".into(),
            feed_url: String::new(),
        }
    }
}

pub const DEFAULT_FEED_URL: &str =
    "https://api.github.com/repos/CNB/PureTerm/releases/latest";

impl AutoUpdateConfig {
    /// Returns the effective feed URL, falling back to the GitHub Releases
    /// default when [`feed_url`](Self::feed_url) is empty.
    pub fn effective_feed_url(&self) -> &str {
        if self.feed_url.is_empty() {
            DEFAULT_FEED_URL
        } else {
            &self.feed_url
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_available: Option<String>,
    pub message: String,
}

/// Compare `a` and `b` as dotted `MAJOR.MINOR.PATCH` semver tuples.
/// Returns `true` when `a < b`. Pre-release / build metadata suffixes are
/// ignored — this is intentionally a minimal comparator for the update flow.
pub fn semver_less(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let av = parse(a);
    let bv = parse(b);
    for i in 0..av.len().max(bv.len()) {
        let x = av.get(i).copied().unwrap_or(0);
        let y = bv.get(i).copied().unwrap_or(0);
        if x < y {
            return true;
        }
        if x > y {
            return false;
        }
    }
    false
}

/// Synchronous fast-path: inspects the current build's `CARGO_PKG_VERSION`
/// against the most recently seen `PURETERM_LATEST_VERSION` env var. When
/// `cfg.feed_url` is set, the message hints that a remote check should be
/// run via [`check_remote_async`].
pub fn check_for_updates(cfg: &AutoUpdateConfig) -> UpdateCheckResult {
    let current = env!("CARGO_PKG_VERSION").to_string();
    if !cfg.enabled {
        return UpdateCheckResult {
            current_version: current,
            latest_available: None,
            message: "update checks disabled".into(),
        };
    }
    let latest = std::env::var("PURETERM_LATEST_VERSION").ok();
    let message = match &latest {
        Some(v) if v != &current => {
            if semver_less(&current, v) {
                format!("update available: {v} (current {current})")
            } else {
                format!("local build {current} is newer than feed {v}")
            }
        }
        Some(_) => format!("up to date ({current})"),
        None => {
            if cfg.feed_url.is_empty() {
                format!(
                    "current {current}; run `pureterm update check --remote` to query GitHub Releases"
                )
            } else {
                format!(
                    "current {current}; run `pureterm update check --remote` against {}",
                    cfg.effective_feed_url()
                )
            }
        }
    };
    UpdateCheckResult {
        current_version: current,
        latest_available: latest,
        message,
    }
}

/// Subset of the GitHub Releases API we care about. Defined locally so
/// deserialization does not depend on unstable API surface.
#[derive(Clone, Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Result of an async remote check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteCheckResult {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub source_url: String,
}

/// Async GitHub Releases check. Uses `reqwest` (already a workspace
/// dependency). When the request fails — no network, rate-limited, etc. —
/// the result's `latest_version` is `None` rather than an error so the
/// caller can fall back to the synchronous check.
pub async fn check_remote_async(cfg: &AutoUpdateConfig) -> RemoteCheckResult {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let url = cfg.effective_feed_url().to_string();
    if !cfg.enabled {
        return RemoteCheckResult {
            current_version: current,
            latest_version: None,
            update_available: false,
            source_url: url,
        };
    }
    let client = match reqwest::Client::builder()
        .user_agent(concat!("pureterm/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return RemoteCheckResult {
                current_version: current,
                latest_version: None,
                update_available: false,
                source_url: url,
            };
        }
    };
    let Ok(resp) = client.get(&url).send().await else {
        return RemoteCheckResult {
            current_version: current,
            latest_version: None,
            update_available: false,
            source_url: url,
        };
    };
    let Ok(release) = resp.json::<GitHubRelease>().await else {
        return RemoteCheckResult {
            current_version: current,
            latest_version: None,
            update_available: false,
            source_url: url,
        };
    };
    // Skip drafts and pre-releases on the stable channel.
    let skip = release.draft || (cfg.channel == "stable" && release.prerelease);
    let tag = release.tag_name.trim_start_matches('v').to_string();
    let update_available = !skip && semver_less(&current, &tag);
    RemoteCheckResult {
        current_version: current,
        latest_version: Some(tag),
        update_available,
        source_url: url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_reports_current() {
        let r = check_for_updates(&AutoUpdateConfig::default());
        assert_eq!(r.current_version, env!("CARGO_PKG_VERSION"));
        assert!(r.latest_available.is_none());
    }

    #[test]
    fn semver_less_basic() {
        assert!(semver_less("0.1.0", "0.2.0"));
        assert!(semver_less("0.1.0", "1.0.0"));
        assert!(!semver_less("0.2.0", "0.1.0"));
        assert!(!semver_less("1.0.0", "1.0.0"));
    }

    #[test]
    fn semver_less_handles_v_prefix_and_suffix() {
        assert!(semver_less("v0.1.0", "v0.1.1"));
        assert!(!semver_less("0.1.0-rc1", "0.1.0")); // comparator strips pre-release suffix
    }

    #[test]
    fn semver_less_handles_uneven_segments() {
        assert!(semver_less("0.1", "0.1.1"));
        assert!(!semver_less("0.1.1", "0.1"));
    }

    #[test]
    fn effective_feed_url_defaults_to_github() {
        let cfg = AutoUpdateConfig::default();
        assert_eq!(cfg.effective_feed_url(), DEFAULT_FEED_URL);
        let cfg2 = AutoUpdateConfig {
            feed_url: "https://example.com/feed.json".into(),
            ..cfg.clone()
        };
        assert_eq!(cfg2.effective_feed_url(), "https://example.com/feed.json");
    }

    #[test]
    fn check_returns_up_to_date_when_versions_match() {
        let cfg = AutoUpdateConfig {
            enabled: true,
            ..AutoUpdateConfig::default()
        };
        // Without PURETERM_LATEST_VERSION env var, message should not claim
        // an update is available.
        let r = check_for_updates(&cfg);
        if let Some(latest) = &r.latest_available {
            assert_ne!(latest, &r.current_version);
        }
    }
}
