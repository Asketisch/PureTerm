use alacritty_config_derive::SerdeReplace;
use pureterm_ssh::SshForwards;
use serde::{Deserialize, Serialize};

use crate::password_manager::PasswordRef;
use crate::serial_telnet::{SerialSettings, SessionType};

/// PureTerm 的 SSH 连接配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, SerdeReplace)]
pub struct SshProfile {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>, // 已加密
    pub key_path: Option<String>,
    pub shortcut: Option<String>,
    /// GAP-5.4:把本地 ssh-agent 转发给远端主机(以
    /// `-o ForwardAgent=yes` 形式)。默认关闭 —— agent 转发在
    /// 不可信 / 被攻陷的主机上是信任风险(OpenSSH 同样默认关闭)。
    #[serde(default)]
    pub forward_agent: bool,
    /// GAP-40:本地端口转发(`ssh -L`),格式为 `localPort:remoteHost:remotePort`。
    #[serde(default)]
    pub local_forwards: Vec<String>,
    /// GAP-40:远端端口转发(`ssh -R`)。
    #[serde(default)]
    pub remote_forwards: Vec<String>,
    /// GAP-40:不受信任的 X11 转发(`ssh -X`)。
    #[serde(default)]
    pub forward_x11: bool,
    /// GAP-40:受信任的 X11 转发(`ssh -Y`;覆盖 `forward_x11`)。
    #[serde(default)]
    pub forward_x11_trusted: bool,
    /// GAP-41:跳板机链(`ProxyJump`);先于 OpenSSH 配置尝试。
    #[serde(default)]
    pub jump_hosts: Vec<String>,
    /// GAP-42:登录后运行的命令(每行一条)。
    #[serde(default)]
    pub login_commands: Vec<String>,
    /// GAP-42:登录命令之间的延迟(毫秒)。
    #[serde(default = "default_login_delay_ms")]
    pub login_delay_ms: u64,
    /// GAP-42:首次失败时 `continue` 或 `stop`。
    #[serde(default)]
    pub login_on_error: String,
    /// GAP-37:1Password 条目引用(`op://…`)或 LastPass 条目名称。
    #[serde(default)]
    pub password_ref: Option<PasswordRefToml>,
    /// GAP-39:`ssh`(默认)、`telnet` 或 `serial`。
    #[serde(default)]
    pub session_type: String,
    /// GAP-39:串口设备路径(例如 `/dev/ttyUSB0`)。
    #[serde(default)]
    pub serial_port: Option<String>,
    /// GAP-39:串口波特率。
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

/// 适合 TOML 的密码引用(扁平的)。
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

    /// GAP-40:为 `SshCommandBuilder` 构建转发集合。
    pub fn forwards(&self) -> SshForwards {
        SshForwards {
            local: self.local_forwards.clone(),
            remote: self.remote_forwards.clone(),
            x11: self.forward_x11,
            x11_trusted: self.forward_x11_trusted,
        }
    }
}

/// 从配置加载的 SSH 连接配置注册表
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

/// 全局 `[ssh]` 表旋钮(非每个 profile)。
#[derive(Debug, Clone)]
pub struct SshSettings {
    /// GAP-5.3:多个窗格附加同一个守护进程会话时,PTY
    /// 缩放使用所有附加窗格中 min(cols) × min(rows)。
    pub smallest_screen_wins: bool,
    /// GAP-37:密码管理器来源(`auto`、`keychain`、`1password`、`lastpass`)。
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

/// 从 `pureterm.toml` 加载 `[ssh]` 设置。表缺失 ⇒ 使用默认值。
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
