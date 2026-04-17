//! SSH X11 转发支持。

use std::net::SocketAddr;

/// SSH 通道上 X11 转发的配置。
#[derive(Debug, Clone)]
pub struct X11ForwardConfig {
    /// X11 认证协议(通常为 "MIT-MAGIC-COOKIE-1")。
    pub auth_protocol: String,
    /// X11 认证 cookie 数据。
    pub auth_cookie: Vec<u8>,
    /// 屏幕编号。
    pub screen_number: u32,
    /// 单连接模式 — 只有一个客户端可以连接。
    pub single_connection: bool,
}

impl Default for X11ForwardConfig {
    fn default() -> Self {
        Self {
            auth_protocol: "MIT-MAGIC-COOKIE-1".to_string(),
            auth_cookie: Vec::new(),
            screen_number: 0,
            single_connection: false,
        }
    }
}

/// X11 转发通道的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11State {
    /// X11 转发已请求但尚未确认。
    Pending,
    /// X11 转发处于活跃状态。
    Active,
    /// X11 转发被服务器拒绝。
    Rejected,
    /// X11 转发已关闭。
    Closed,
}

/// 管理 SSH 会话的 X11 转发。
pub struct X11ForwardManager {
    config: X11ForwardConfig,
    state: X11State,
    /// X11 显示的本地地址(例如,来自 $DISPLAY)。
    local_display: Option<SocketAddr>,
}

impl X11ForwardManager {
    pub fn new(config: X11ForwardConfig) -> Self {
        Self {
            config,
            state: X11State::Pending,
            local_display: None,
        }
    }

    /// 解析 DISPLAY 环境变量并解析为套接字地址。
    pub fn resolve_display() -> Option<SocketAddr> {
        let display = std::env::var("DISPLAY").ok()?;
        // DISPLAY 格式: hostname:displaynumber.screennumber
        // 或 :displaynumber.screennumber (本地)
        let display = display.trim_start_matches(':');
        let parts: Vec<&str> = display.split('.').collect();
        let display_num: u32 = parts.first()?.parse().ok()?;

        // TCP 端口 = 6000 + 显示编号
        let port = 6000 + display_num;
        Some(SocketAddr::from(([127, 0, 0, 1], port as u16)))
    }

    /// 使用自动检测的显示设置创建管理器。
    pub fn from_env() -> Self {
        let display = Self::resolve_display();
        let mut config = X11ForwardConfig::default();
        // 生成随机认证 cookie
        config.auth_cookie = random_bytes(16);
        Self {
            config,
            state: X11State::Pending,
            local_display: display,
        }
    }

    /// 为通道请求构建 X11 认证 cookie 字符串。
    pub fn auth_cookie_hex(&self) -> String {
        self.config.auth_cookie.iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }

    pub fn state(&self) -> X11State {
        self.state
    }

    pub fn set_state(&mut self, state: X11State) {
        self.state = state;
    }

    pub fn config(&self) -> &X11ForwardConfig {
        &self.config
    }

    pub fn local_display(&self) -> Option<SocketAddr> {
        self.local_display
    }
}

// 使用系统时间的简单随机字节生成
fn random_bytes(len: usize) -> Vec<u8> {
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let mut state = seed;
    (0..len).map(|_| {
        // xorshift64
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state & 0xFF) as u8
    }).collect()
}
