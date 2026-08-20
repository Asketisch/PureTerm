//! SSH agent socket(`$SSH_AUTH_SOCK`)解析——GAP-5.4。
//!
//! PureTerm 常由 GUI 启动(Finder/Dock),启动它的 shell 的
//! `SSH_AUTH_SOCK` 通常不在进程环境里。为了让
//! agent 转发仍能工作,解析按以下顺序尝试:
//!   1. 显式配置覆盖(`ssh.agent_sock`),
//!   2. 继承的 `$SSH_AUTH_SOCK` 环境变量,
//!   3. macOS 上,`launchctl getenv SSH_AUTH_SOCK`(GUI 应用的标准修法)。
//!
//! 优先级逻辑位于 [`resolve_pure`](纯函数,带单元测试);
//! 薄封装 [`resolve_agent_sock`] 读取真实环境。

use std::path::{Path, PathBuf};

/// 纯优先级逻辑(不访问环境)——带单元测试。
///
/// 顺序:显式 `config_override` → 继承的 `env` → `launchctl` 回退。
/// 空字符串 / 空路径视为缺失,落入下一级。
pub(crate) fn resolve_pure(
    env: Option<&str>,
    launchctl: Option<String>,
    config_override: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(p) = config_override.filter(|p| !p.as_os_str().is_empty()) {
        return Some(p.to_path_buf());
    }
    if let Some(p) = env.filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(p));
    }
    launchctl.filter(|s| !s.is_empty()).map(PathBuf::from)
}

/// 为当前进程解析 SSH agent socket。
///
/// 传入显式配置覆盖路径(`ssh.agent_sock`)可强制指定
/// socket;否则使用继承的 `$SSH_AUTH_SOCK`,macOS 上对 GUI
/// 启动的进程再回退到 `launchctl getenv`。
pub fn resolve_agent_sock(config_override: Option<&Path>) -> Option<PathBuf> {
    let env = std::env::var("SSH_AUTH_SOCK").ok();
    let launchctl = launchctl_getenv();
    resolve_pure(env.as_deref(), launchctl, config_override)
}

/// macOS 上,从 `launchctl getenv` 读取 `SSH_AUTH_SOCK`(GUI 应用
/// 不继承启动 shell 的环境)。其它平台返回 `None`。
#[cfg(target_os = "macos")]
fn launchctl_getenv() -> Option<String> {
    std::process::Command::new("launchctl")
        .args(["getenv", "SSH_AUTH_SOCK"])
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o.stdout) } else { None })
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(not(target_os = "macos"))]
fn launchctl_getenv() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn config_override_wins_over_env_and_launchctl() {
        let r = resolve_pure(
            Some("/from/env"),
            Some("/from/launchctl".to_string()),
            Some(Path::new("/from/config")),
        );
        assert_eq!(r, Some(PathBuf::from("/from/config")));
    }

    #[test]
    fn env_wins_over_launchctl_when_no_override() {
        let r = resolve_pure(Some("/from/env"), Some("/from/launchctl".to_string()), None);
        assert_eq!(r, Some(PathBuf::from("/from/env")));
    }

    #[test]
    fn launchctl_used_when_no_override_or_env() {
        let r = resolve_pure(None, Some("/from/launchctl".to_string()), None);
        assert_eq!(r, Some(PathBuf::from("/from/launchctl")));
    }

    #[test]
    fn empty_env_falls_through_to_launchctl() {
        let r = resolve_pure(Some(""), Some("/from/launchctl".to_string()), None);
        assert_eq!(r, Some(PathBuf::from("/from/launchctl")));
    }

    #[test]
    fn empty_override_falls_through_to_env() {
        let r = resolve_pure(Some("/from/env"), None, Some(Path::new("")));
        assert_eq!(r, Some(PathBuf::from("/from/env")));
    }

    #[test]
    fn empty_launchctl_falls_through_to_none() {
        let r = resolve_pure(None, Some(String::new()), None);
        assert_eq!(r, None);
    }

    #[test]
    fn all_absent_returns_none() {
        let r = resolve_pure(None, None, None);
        assert_eq!(r, None);
    }
}
