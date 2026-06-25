//! SSH agent socket (`$SSH_AUTH_SOCK`) resolution — GAP-5.4.
//!
//! PureTerm is often GUI-launched (Finder/Dock), so the launching shell's
//! `SSH_AUTH_SOCK` is usually absent from the process environment. To make
//! agent forwarding work nonetheless, resolution tries, in order:
//!   1. an explicit config override (`ssh.agent_sock`),
//!   2. the inherited `$SSH_AUTH_SOCK` environment variable,
//!   3. on macOS, `launchctl getenv SSH_AUTH_SOCK` (the standard GUI-app fix).
//!
//! The precedence logic lives in [`resolve_pure`] (pure, unit-tested); the
//! thin [`resolve_agent_sock`] wrapper reads the real environment.

use std::path::{Path, PathBuf};

/// Pure precedence logic (no environment access) — unit-tested.
///
/// Order: explicit `config_override` → inherited `env` → `launchctl` fallback.
/// Empty strings / empty paths are treated as absent and fall through.
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

/// Resolve the SSH agent socket for the current process.
///
/// Pass an explicit config override path (`ssh.agent_sock`) to force a
/// specific socket; otherwise the inherited `$SSH_AUTH_SOCK` is used, with a
/// `launchctl getenv` fallback on macOS for GUI-launched processes.
pub fn resolve_agent_sock(config_override: Option<&Path>) -> Option<PathBuf> {
    let env = std::env::var("SSH_AUTH_SOCK").ok();
    let launchctl = launchctl_getenv();
    resolve_pure(env.as_deref(), launchctl, config_override)
}

/// On macOS, read `SSH_AUTH_SOCK` from `launchctl getenv` (GUI apps don't
/// inherit the launching shell's environment). Returns `None` elsewhere.
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
