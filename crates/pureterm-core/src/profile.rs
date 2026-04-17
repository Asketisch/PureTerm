use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileType {
    Ssh,
    Serial,
    Local,
    Telnet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub profile_type: ProfileType,
    pub group: Option<String>,
    pub options: ProfileOptions,
    #[serde(default)]
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileOptions {
    // 本地
    pub shell: Option<String>,
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub args: Option<Vec<String>>,

    // SSH
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub auth_method: Option<String>,
    pub private_key: Option<String>,
    pub agent_forward: Option<bool>,
    pub x11_forward: Option<bool>,
    pub compression: Option<bool>,

    // 串口
    pub path: Option<String>,
    pub baud_rate: Option<u32>,
    pub data_bits: Option<u8>,
    pub stop_bits: Option<u8>,
    pub parity: Option<String>,
    pub flow_control: Option<String>,

    // Telnet
    pub telnet_host: Option<String>,
    pub telnet_port: Option<u16>,
}

impl Default for ProfileOptions {
    fn default() -> Self {
        Self {
            shell: None,
            cwd: None,
            env: None,
            args: None,
            host: None,
            port: None,
            user: None,
            auth_method: None,
            private_key: None,
            agent_forward: None,
            x11_forward: None,
            compression: None,
            path: None,
            baud_rate: None,
            data_bits: None,
            stop_bits: None,
            parity: None,
            flow_control: None,
            telnet_host: None,
            telnet_port: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileGroup {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

pub struct TabParams {
    pub profile_type: ProfileType,
    pub title: String,
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
}

pub trait ProfileProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn create_tab_params(&self, profile: &Profile) -> Result<TabParams>;
    fn get_description(&self, profile: &Profile) -> String;
    fn quick_connect(&self, query: &str) -> Result<Profile>;
}

pub struct ProfileManager {
    providers: HashMap<&'static str, Box<dyn ProfileProvider>>,
}

impl ProfileManager {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register_provider(&mut self, provider: Box<dyn ProfileProvider>) {
        self.providers.insert(provider.id(), provider);
    }

    pub fn create_tab_params(&self, profile: &Profile) -> Result<TabParams> {
        let type_str = match profile.profile_type {
            ProfileType::Ssh => "ssh",
            ProfileType::Serial => "serial",
            ProfileType::Local => "local",
            ProfileType::Telnet => "telnet",
        };
        let provider = self.providers.get(type_str)
            .ok_or_else(|| crate::error::CoreError::ProfileNotFound(type_str.into()))?;
        provider.create_tab_params(profile)
    }

    pub fn get_description(&self, profile: &Profile) -> String {
        let type_str = match profile.profile_type {
            ProfileType::Ssh => "ssh",
            ProfileType::Serial => "serial",
            ProfileType::Local => "local",
            ProfileType::Telnet => "telnet",
        };
        if let Some(provider) = self.providers.get(type_str) {
            provider.get_description(profile)
        } else {
            format!("{:?}", profile.profile_type)
        }
    }
}

impl Default for ProfileManager {
    fn default() -> Self {
        Self::new()
    }
}
