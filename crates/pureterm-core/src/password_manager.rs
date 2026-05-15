//! Password Manager — provider-based backend for fetching secrets on demand.
//!
//! Modeled on iTerm2's `sources/PasswordManager/`. Stores a list of
//! [`PasswordEntry`] descriptors; each entry resolves to a secret by invoking
//! its [`PasswordProvider`] (Keychain via `/usr/bin/security`, 1Password CLI,
//! or an arbitrary shell command). The pane will later call [`PasswordManager::fetch`]
//! to satisfy a detected password prompt without exposing the secret in scrollback.
//!
//! Note: this is the backend only — UI integration lands in a follow-up commit.

use serde::{Deserialize, Serialize};
use std::io::{Error, ErrorKind, Result};
use std::process::Command;

/// Source from which a password is retrieved.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum PasswordProvider {
    /// macOS Keychain via `/usr/bin/security find-generic-password`.
    Keychain { account: String, service: String },
    /// 1Password CLI: `op read op://vault/item/field`.
    OnePassword { reference: String },
    /// Generic command that prints the password to stdout.
    Command { command: String },
}

/// A user-facing password descriptor stored in config.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PasswordEntry {
    pub id: String,
    pub label: String,
    pub provider: PasswordProvider,
}

/// In-memory collection of password entries with provider-driven fetch.
#[derive(Clone, Debug, Default)]
pub struct PasswordManager {
    entries: Vec<PasswordEntry>,
}

impl PasswordManager {
    pub fn new(entries: Vec<PasswordEntry>) -> Self {
        Self { entries }
    }

    pub fn list(&self) -> &[PasswordEntry] {
        &self.entries
    }

    pub fn add(&mut self, entry: PasswordEntry) {
        self.entries.push(entry);
    }

    /// Remove the entry with the given id; returns `true` if one was removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    /// Look up an entry by id and run its provider, returning the secret on stdout.
    pub fn fetch(&self, id: &str) -> Result<String> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("no password entry: {id}")))?;
        run_provider(&entry.provider)
    }
}

fn run_provider(provider: &PasswordProvider) -> Result<String> {
    let output = match provider {
        PasswordProvider::Keychain { account, service } => Command::new("/usr/bin/security")
            .args(["find-generic-password", "-a", account, "-s", service, "-w"])
            .output()?,
        PasswordProvider::OnePassword { reference } => {
            Command::new("op").args(["read", reference]).output()?
        }
        PasswordProvider::Command { command } => {
            Command::new("sh").arg("-c").arg(command).output()?
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::new(
            ErrorKind::Other,
            format!("password provider failed: {stderr}"),
        ));
    }

    let mut secret = String::from_utf8_lossy(&output.stdout).into_owned();
    while secret.ends_with('\n') || secret.ends_with('\r') {
        secret.pop();
    }
    if secret.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "password provider returned empty stdout",
        ));
    }
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> PasswordEntry {
        PasswordEntry {
            id: id.into(),
            label: format!("label-{id}"),
            provider: PasswordProvider::Keychain {
                account: "alice".into(),
                service: "github.com".into(),
            },
        }
    }

    #[test]
    fn add_and_remove_entry() {
        let mut mgr = PasswordManager::new(Vec::new());
        mgr.add(sample("a"));
        mgr.add(sample("b"));
        assert_eq!(mgr.list().len(), 2);
        assert!(mgr.remove("a"));
        assert_eq!(mgr.list().len(), 1);
        assert_eq!(mgr.list()[0].id, "b");
        assert!(!mgr.remove("missing"));
    }

    #[test]
    fn list_returns_added_entries() {
        let mgr = PasswordManager::new(vec![sample("x"), sample("y")]);
        let ids: Vec<&str> = mgr.list().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["x", "y"]);
    }

    #[test]
    fn serde_roundtrip_keychain() {
        let entry = PasswordEntry {
            id: "kc1".into(),
            label: "GitHub".into(),
            provider: PasswordProvider::Keychain {
                account: "alice".into(),
                service: "github.com".into(),
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: PasswordEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }

    #[test]
    fn serde_roundtrip_onepassword() {
        let entry = PasswordEntry {
            id: "op1".into(),
            label: "AWS".into(),
            provider: PasswordProvider::OnePassword {
                reference: "op://Personal/AWS/password".into(),
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: PasswordEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }

    #[test]
    fn serde_roundtrip_command() {
        let entry = PasswordEntry {
            id: "cmd1".into(),
            label: "Custom".into(),
            provider: PasswordProvider::Command {
                command: "pass show personal/email".into(),
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: PasswordEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }

    #[cfg(unix)]
    #[test]
    fn fetch_command_provider_runs_shell() {
        let mgr = PasswordManager::new(vec![PasswordEntry {
            id: "hello".into(),
            label: "hello".into(),
            provider: PasswordProvider::Command {
                command: "printf hello".into(),
            },
        }]);
        let secret = mgr.fetch("hello").expect("fetch should succeed");
        assert_eq!(secret, "hello");
    }

    #[test]
    fn fetch_unknown_id_errors() {
        let mgr = PasswordManager::new(vec![sample("known")]);
        let err = mgr.fetch("nope").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }
}
