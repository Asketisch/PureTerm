use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::broadcast;

/// 用于创建新会话连接的参数。
#[derive(Debug, Clone)]
pub struct ConnectParams {
    pub session_type: SessionType,
    pub env: std::collections::HashMap<String, String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SessionType {
    Local {
        shell: Option<String>,
        args: Vec<String>,
    },
    Ssh {
        host: String,
        port: u16,
        user: String,
        auth: SshAuth,
        agent_forward: bool,
        x11_forward: bool,
        compression: bool,
    },
    Serial {
        path: String,
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: String,
        flow_control: String,
    },
    Telnet {
        host: String,
        port: u16,
    },
}

#[derive(Debug, Clone)]
pub enum SshAuth {
    PromptPassword,
    Password(String),
    PublicKey {
        key_path: std::path::PathBuf,
        passphrase: Option<String>,
    },
    Agent,
    None,
}

/// 所有会话类型的 trait。
#[async_trait]
pub trait Session: Send {
    /// 检查会话是否仍然打开。
    fn is_open(&self) -> bool;

    /// 启动会话。
    async fn start(&mut self) -> Result<()>;

    /// 调整 PTY/通道大小。
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;

    /// 向会话写入数据。
    fn write(&mut self, data: &[u8]) -> Result<()>;

    /// 向进程发送信号。
    fn kill(&mut self, signal: &str) -> Result<()>;

    /// 订阅会话的输出数据。
    /// 返回一个接收器,会接收所有会话输出。
    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>>;

    /// 获取会话类型名称。
    fn session_type_name(&self) -> &'static str;
}
