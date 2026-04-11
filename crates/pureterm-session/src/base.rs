use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::broadcast;

/// Parameters for creating a new session connection.
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

/// Trait for all session types.
#[async_trait]
pub trait Session: Send {
    /// Check if the session is still open.
    fn is_open(&self) -> bool;

    /// Start the session.
    async fn start(&mut self) -> Result<()>;

    /// Resize the PTY/channel.
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;

    /// Write data to the session.
    fn write(&mut self, data: &[u8]) -> Result<()>;

    /// Send a signal to the process.
    fn kill(&mut self, signal: &str) -> Result<()>;

    /// Subscribe to output data from the session.
    /// Returns a receiver that will receive all session output.
    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>>;

    /// Get the session type name.
    fn session_type_name(&self) -> &'static str;
}
