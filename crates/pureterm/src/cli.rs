use clap::{Parser, Subcommand};
use pureterm_core::profile::{Profile, ProfileType, ProfileOptions};
use pureterm_session::base::{ConnectParams, SessionType, SshAuth};

/// PureTerm CLI 参数。
#[derive(Parser, Debug)]
#[command(name = "pureterm")]
#[command(version = "0.1.0")]
#[command(about = "GPU-accelerated terminal emulator")]
pub(crate) struct Cli {
    /// 通过名称连接到配置文件。
    #[arg(short, long)]
    pub(crate) profile: Option<String>,

    /// 快速 SSH 连接（user@host 或 user@host:port）。
    #[arg(short, long)]
    pub(crate) ssh: Option<String>,

    /// 打开新标签页（用于 IPC）。
    #[arg(long)]
    pub(crate) new_tab: bool,

    /// 启动 quake 模式（下拉终端）。
    #[arg(long)]
    pub(crate) quake: bool,

    /// 用于额外操作的子命令。
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// 列出可用配置文件。
    List,
    /// 打开设置。
    Settings,
    /// 列出所有可用字体。
    ListFonts,
    /// 列出所有可用主题。
    ListThemes,
    /// 显示当前配置。
    ShowConfig,
    /// 显示版本信息。
    Version,
}

/// 将 SSH 规范字符串（user@host:port）解析为 Profile。
pub(crate) fn parse_ssh_spec(spec: &str) -> Option<Profile> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }

    let (user, host_part) = if let Some(at) = spec.find('@') {
        (Some(&spec[..at]), &spec[at + 1..])
    } else {
        (None, spec)
    };

    let (host, port) = if let Some(colon) = host_part.find(':') {
        let port: u16 = host_part[colon + 1..].parse().ok()?;
        (host_part[..colon].to_string(), Some(port))
    } else {
        (host_part.to_string(), None)
    };

    let user = user.map(|s| s.to_string())
        .or_else(|| std::env::var("USER").ok());

    Some(Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: format!("{}@{}", user.as_deref().unwrap_or("user"), host),
        profile_type: ProfileType::Ssh,
        group: None,
        color: Some("#4A9EFF".to_string()),
        icon: None,
        options: ProfileOptions {
            host: Some(host),
            port,
            user,
            auth_method: Some("prompt".to_string()),
            ..Default::default()
        },
    })
}

/// 将 Profile 转换为 ConnectParams 以创建会话。
pub(crate) fn profile_to_connect_params(profile: &Profile) -> ConnectParams {
    let session_type = match profile.profile_type {
        ProfileType::Local => {
            SessionType::Local {
                shell: profile.options.shell.clone(),
                args: profile.options.args.clone().unwrap_or_default(),
            }
        }
        ProfileType::Ssh => {
            let auth = match profile.options.auth_method.as_deref() {
                Some("password") => SshAuth::Password(String::new()),
                Some("publickey") => {
                    let key_path = profile.options.private_key.clone()
                        .unwrap_or_else(|| "~/.ssh/id_rsa".to_string());
                    SshAuth::PublicKey {
                        key_path: std::path::PathBuf::from(key_path),
                        passphrase: None,
                    }
                }
                Some("agent") => SshAuth::Agent,
                Some("none") => SshAuth::None,
                _ => SshAuth::PromptPassword,
            };
            SessionType::Ssh {
                host: profile.options.host.clone().unwrap_or_default(),
                port: profile.options.port.unwrap_or(22),
                user: profile.options.user.clone().unwrap_or_default(),
                auth,
                agent_forward: profile.options.agent_forward.unwrap_or(false),
                x11_forward: profile.options.x11_forward.unwrap_or(false),
                compression: profile.options.compression.unwrap_or(false),
            }
        }
        ProfileType::Serial => {
            SessionType::Serial {
                path: profile.options.path.clone().unwrap_or_default(),
                baud_rate: profile.options.baud_rate.unwrap_or(9600),
                data_bits: profile.options.data_bits.unwrap_or(8),
                stop_bits: profile.options.stop_bits.unwrap_or(1),
                parity: profile.options.parity.clone().unwrap_or_else(|| "none".into()),
                flow_control: profile.options.flow_control.clone().unwrap_or_else(|| "none".into()),
            }
        }
        ProfileType::Telnet => {
            SessionType::Telnet {
                host: profile.options.telnet_host.clone().unwrap_or_default(),
                port: profile.options.telnet_port.unwrap_or(23),
            }
        }
    };

    ConnectParams {
        session_type,
        env: profile.options.env.clone().unwrap_or_default(),
        cwd: profile.options.cwd.clone(),
    }
}

/// 从配置文件生成标签页标题。
pub(crate) fn profile_title(profile: &Profile) -> String {
    match profile.profile_type {
        ProfileType::Ssh => {
            let user = profile.options.user.as_deref().unwrap_or("user");
            let host = profile.options.host.as_deref().unwrap_or("host");
            let port = profile.options.port.unwrap_or(22);
            if port == 22 { format!("{}@{}", user, host) } else { format!("{}@{}:{}", user, host, port) }
        }
        ProfileType::Serial => {
            let path = profile.options.path.as_deref().unwrap_or("/dev/tty");
            let baud = profile.options.baud_rate.unwrap_or(9600);
            format!("{} @ {}", path, baud)
        }
        ProfileType::Telnet => {
            let host = profile.options.telnet_host.as_deref().unwrap_or("host");
            let port = profile.options.telnet_port.unwrap_or(23);
            if port == 23 { host.to_string() } else { format!("{}:{}", host, port) }
        }
        ProfileType::Local => profile.name.clone(),
    }
}
