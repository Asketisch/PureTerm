use alacritty_config_derive::SerdeReplace;
use pureterm_ssh::SshForwards;
use serde::{Deserialize, Serialize};

use crate::password_manager::PasswordRef;
use crate::serial_telnet::{SerialSettings, SessionType};

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
    /// GAP-5.4: forward the local ssh-agent to the remote host (`-o
    /// ForwardAgent=yes`). Default off — agent forwarding is a trust risk on
    /// untrusted/compromised hosts (OpenSSH defaults off for the same reason).
    #[serde(default)]
    pub forward_agent: bool,
    /// GAP-40: local port forwards (`ssh -L`), each `localPort:remoteHost:remotePort`.
    #[serde(default)]
    pub local_forwards: Vec<String>,
    /// GAP-40: remote port forwards (`ssh -R`).
    #[serde(default)]
    pub remote_forwards: Vec<String>,
    /// GAP-40: untrusted X11 forwarding (`ssh -X`).
    #[serde(default)]
    pub forward_x11: bool,
    /// GAP-40: trusted X11 forwarding (`ssh -Y`; overrides `forward_x11`).
    #[serde(default)]
    pub forward_x11_trusted: bool,
    /// GAP-41: jump host chain (`ProxyJump`); tried before OpenSSH config.
    #[serde(default)]
    pub jump_hosts: Vec<String>,
    /// GAP-42: commands to run after login (one per line).
    #[serde(default)]
    pub login_commands: Vec<String>,
    /// GAP-42: delay between login commands (ms).
    #[serde(default = "default_login_delay_ms")]
    pub login_delay_ms: u64,
    /// GAP-42: `continue` or `stop` on first failure.
    #[serde(default)]
    pub login_on_error: String,
    /// GAP-37: 1Password item ref (`op://…`) or LastPass entry name.
    #[serde(default)]
    pub password_ref: Option<PasswordRefToml>,
    /// GAP-39: `ssh` (default), `telnet`, or `serial`.
    #[serde(default)]
    pub session_type: String,
    /// GAP-39: serial device path (e.g. `/dev/ttyUSB0`).
    #[serde(default)]
    pub serial_port: Option<String>,
    /// GAP-39: serial baud rate.
    #[serde(default)]
    pub baud_rate: Option<u32>,
}

fn default_login_delay_ms() -> u64 {
    500
}

impl Default for SshProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: String::new(),
            user: None,
            port: None,
            password: None,
            key_path: None,
            shortcut: None,
            forward_agent: false,
            local_forwards: Vec::new(),
            remote_forwards: Vec::new(),
            forward_x11: false,
            forward_x11_trusted: false,
            jump_hosts: Vec::new(),
            login_commands: Vec::new(),
            login_delay_ms: default_login_delay_ms(),
            login_on_error: String::new(),
            password_ref: None,
            session_type: "ssh".to_string(),
            serial_port: None,
            baud_rate: None,
        }
    }
}

/// TOML-friendly password ref (flattened).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, SerdeReplace)]
pub struct PasswordRefToml {
    #[serde(default)]
    pub op_ref: Option<String>,
    #[serde(default)]
    pub lpass_entry: Option<String>,
}

impl PasswordRefToml {
    pub fn to_ref(&self) -> PasswordRef {
        PasswordRef {
            op_ref: self.op_ref.clone(),
            lpass_entry: self.lpass_entry.clone(),
        }
    }
}

impl SshProfile {
    pub fn session_type(&self) -> SessionType {
        SessionType::parse(&self.session_type)
    }

    pub fn serial_settings(&self) -> SerialSettings {
        SerialSettings {
            port: self
                .serial_port
                .clone()
                .unwrap_or_else(|| "/dev/ttyUSB0".into()),
            baud: self.baud_rate.unwrap_or(115200),
            ..SerialSettings::default()
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }
    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }
    pub fn port(&self) -> u16 {
        self.port.unwrap_or(22)
    }

    /// GAP-40: build the forward bundle for `SshCommandBuilder`.
    pub fn forwards(&self) -> SshForwards {
        SshForwards {
            local: self.local_forwards.clone(),
            remote: self.remote_forwards.clone(),
            x11: self.forward_x11,
            x11_trusted: self.forward_x11_trusted,
        }
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

/// Global `[ssh]` table knobs (not per-profile).
#[derive(Debug, Clone)]
pub struct SshSettings {
    /// GAP-5.3: when several panes attach the same daemon session, PTY
    /// resize uses min(cols) × min(rows) across all attached panes.
    pub smallest_screen_wins: bool,
    /// GAP-37: password manager source (`auto`, `keychain`, `1password`, `lastpass`).
    pub password_manager: String,
}

impl Default for SshSettings {
    fn default() -> Self {
        Self {
            smallest_screen_wins: true,
            password_manager: "auto".into(),
        }
    }
}

/// Load `[ssh]` settings from `pureterm.toml`. Missing table ⇒ defaults.
pub fn load_ssh_settings(path: &std::path::Path) -> SshSettings {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return SshSettings::default(),
    };
    let value: toml::Value = match toml::from_str(&contents) {
        Ok(v) => v,
        Err(_) => return SshSettings::default(),
    };
    let Some(ssh) = value.get("ssh") else {
        return SshSettings::default();
    };
    SshSettings {
        smallest_screen_wins: ssh
            .get("smallest_screen_wins")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        password_manager: ssh
            .get("password_manager")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string(),
    }
}