//! GAP-37:密码管理器集成——SSH 用 1Password / LastPass / Keychain。

use std::process::Command;

/// 全局 `[ssh].password_manager` 设置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PasswordManagerSource {
    #[default]
    Auto,
    Keychain,
    OnePassword,
    LastPass,
}

impl PasswordManagerSource {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "keychain" => Self::Keychain,
            "1password" | "op" => Self::OnePassword,
            "lastpass" | "lpass" => Self::LastPass,
            _ => Self::Auto,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PasswordRef {
    pub op_ref: Option<String>,
    pub lpass_entry: Option<String>,
}

/// 探测 PATH 上有哪些 CLI 密码源可用。
pub fn probe_sources() -> Vec<&'static str> {
    let mut out = Vec::new();
    if which("security") {
        out.push("keychain");
    }
    if which("op") {
        out.push("1password");
    }
    if which("lpass") {
        out.push("lastpass");
    }
    out
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 为 SSH 认证解析密码。绝不记录该密钥。
pub fn resolve_password(
    source: PasswordManagerSource,
    host: &str,
    user: Option<&str>,
    port: u16,
    refs: &PasswordRef,
) -> Option<String> {
    let try_order: Vec<PasswordManagerSource> = match source {
        PasswordManagerSource::Auto => {
            let mut v = vec![];
            if which("op") {
                v.push(PasswordManagerSource::OnePassword);
            }
            if which("lpass") {
                v.push(PasswordManagerSource::LastPass);
            }
            if which("security") {
                v.push(PasswordManagerSource::Keychain);
            }
            v
        }
        other => vec![other],
    };

    for src in try_order {
        if let Some(p) = fetch_from(src, host, user, port, refs) {
            return Some(p);
        }
    }
    None
}

fn fetch_from(
    source: PasswordManagerSource,
    host: &str,
    user: Option<&str>,
    port: u16,
    refs: &PasswordRef,
) -> Option<String> {
    match source {
        PasswordManagerSource::OnePassword => {
            let reference = refs.op_ref.clone().unwrap_or_else(|| {
                format!("op://Private/ssh-{host}/password")
            });
            Command::new("op")
                .args(["read", &reference, "--no-newline"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        }
        PasswordManagerSource::LastPass => {
            let entry = refs
                .lpass_entry
                .clone()
                .unwrap_or_else(|| format!("ssh-{host}"));
            Command::new("lpass")
                .args(["show", "-p", &entry])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        }
        PasswordManagerSource::Keychain => {
            let account = user.unwrap_or("root");
            let service = format!("ssh:{host}:{port}");
            Command::new("security")
                .args([
                    "find-generic-password",
                    "-s",
                    &service,
                    "-a",
                    account,
                    "-w",
                ])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        }
        PasswordManagerSource::Auto => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_source() {
        assert_eq!(
            PasswordManagerSource::parse("1password"),
            PasswordManagerSource::OnePassword
        );
    }
}
