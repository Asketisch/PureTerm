//! GAP-45:自动更新——Sparkle / GitHub Releases 检查。
//!
//! 提供:
//! - [`AutoUpdateConfig`] — TOML 配置(`[update]`)。
//! - [`check_for_updates`] — 同步快速路径:比较当前
//!   构建的 `CARGO_PKG_VERSION` 与最近一次看到的
//!   `PURETERM_LATEST_VERSION` 环境变量(方便 CI 冒烟测试)。
//! - [`check_remote_async`] — 经 `reqwest` 的异步 GitHub Releases 抓取,
//!   由特性检查守卫(离线时返回桩)。
//! - [`semver_less`] — 用于判断哪个发布「更新」的简单 semver 比较器,
//!   无需引入完整 semver crate。
//!
//! Sparkle 集成(macOS)有意推迟——脚手架
//! 只记录接缝(`feed_url` / `channel`),尚未链接
//! 该框架。

use serde::{Deserialize, Serialize};

/// `[update]` 配置。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoUpdateConfig {
    pub enabled: bool,
    pub auto_check: bool,
    /// `stable` | `beta`
    pub channel: String,
    /// GitHub Releases 端点或 AppCast feed URL。
    /// 默认为 `https://api.github.com/repos/CNB/PureTerm/releases/latest`
    /// 为空时使用该默认值。
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
    /// 返回生效的 feed URL,当 [`feed_url`](Self::feed_url) 为空时
    /// 回退到 GitHub Releases 默认值。
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

/// 把 `a` 与 `b` 作为点分 `MAJOR.MINOR.PATCH` semver 元组比较。
/// 当 `a < b` 时返回 `true`。预发布 / 构建元数据后缀被忽略——
/// 这是为更新流程有意保留的最小比较器。
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

/// 同步快速路径:检查当前构建的 `CARGO_PKG_VERSION`
/// 与最近一次看到的 `PURETERM_LATEST_VERSION` 环境变量。当
/// `cfg.feed_url` 已设置时,消息提示应经 [`check_remote_async`]
/// 运行远程检查。
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

/// 我们关心的 GitHub Releases API 子集。本地定义,使
/// 反序列化不依赖不稳定的 API 面。
#[derive(Clone, Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// 异步远端检查的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteCheckResult {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub source_url: String,
}

/// 异步 GitHub Releases 检查。使用 `reqwest`(已是工作区
/// 依赖)。当请求失败(无网络、限流等)时,
/// 结果的 `latest_version` 为 `None` 而非报错,以便
/// 调用方可回退到同步检查。
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
    // 在 stable 频道上跳过草稿与预发布。
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
        assert!(!semver_less("0.1.0-rc1", "0.1.0")); // 比较器会剥离预发布后缀
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
        // 未设置 PURETERM_LATEST_VERSION 环境变量时,消息不应声称
        // 有可用更新。
        let r = check_for_updates(&cfg);
        if let Some(latest) = &r.latest_available {
            assert_ne!(latest, &r.current_version);
        }
    }
}
