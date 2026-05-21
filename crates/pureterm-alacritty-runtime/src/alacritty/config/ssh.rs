use alacritty_config_derive::{SerdeReplace};
use serde::{Deserialize, Serialize};
use super::super::ssh::config::SshProfile;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, SerdeReplace)]
pub struct SshConfig {
    #[serde(default = "default_max_idle_minutes")]
    pub max_idle_minutes: u64,
    #[serde(default)]
    pub socket_dir: Option<String>,
    #[serde(default)]
    pub profiles: Vec<SshProfile>,
}

fn default_max_idle_minutes() -> u64 {
    30
}