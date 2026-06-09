use alacritty_config_derive::SerdeReplace;
use serde::{Deserialize, Serialize};

/// SSH Profile for PureTerm (similar to Tabby)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, SerdeReplace)]
pub struct SshProfile {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>, // encrypted
    pub key_path: Option<String>,
    pub shortcut: Option<String>,
}

impl SshProfile {
    pub fn host(&self) -> &str {
        &self.host
    }
    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }
    pub fn port(&self) -> u16 {
        self.port.unwrap_or(22)
    }
}

/// Registry for SSH profiles loaded from config
#[derive(Debug, Clone)]
pub struct SshProfileRegistry {
    profiles: Vec<SshProfile>,
}

impl SshProfileRegistry {
    pub fn new(profiles: Vec<SshProfile>) -> Self {
        Self { profiles }
    }
    pub fn find_by_name(&self, name: &str) -> Option<&SshProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }
    pub fn list_profiles(&self) -> Vec<&SshProfile> {
        self.profiles.iter().collect()
    }
    pub fn len(&self) -> usize {
        self.profiles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

impl Default for SshProfileRegistry {
    fn default() -> Self {
        Self { profiles: Vec::new() }
    }
}