use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// 可发送到 PureTerm 的远程控制命令。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RemoteCommand {
    /// 使用给定配置文件打开新标签页。
    OpenTab {
        profile_name: Option<String>,
        session_type: Option<String>,
    },
    /// 按索引或 ID 关闭标签页。
    CloseTab {
        tab_id: Option<u64>,
        index: Option<usize>,
    },
    /// 发送文本到活动终端。
    SendText { text: String },
    /// 在活动终端中运行命令。
    RunCommand { command: String },
    /// 获取当前状态。
    GetState,
    /// 列出配置文件。
    ListProfiles,
    /// 设置主题。
    SetTheme { name: String },
    /// 切换全屏。
    ToggleFullscreen,
    /// 退出应用程序。
    Quit,
}

impl RemoteCommand {
    /// 从 JSON 字符串解析命令。
    pub fn parse(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("failed to parse RemoteCommand from JSON")
    }

    /// 将此命令序列化为 JSON 字符串。
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("failed to serialize RemoteCommand to JSON")
    }
}

/// 来自 PureTerm 的响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RemoteResponse {
    Ok { message: Option<String> },
    Error { message: String },
    State {
        active_tab: Option<u64>,
        tab_count: usize,
        tabs: Vec<TabInfo>,
    },
    Profiles { profiles: Vec<String> },
}

impl RemoteResponse {
    /// 从 JSON 字符串解析响应。
    pub fn parse(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("failed to parse RemoteResponse from JSON")
    }

    /// 将此响应序列化为 JSON 字符串。
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("failed to serialize RemoteResponse to JSON")
    }
}

/// 关于单个标签页的信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: u64,
    pub title: String,
    pub session_type: String,
}

/// 返回 PureTerm 的平台特定 IPC 套接字路径。
///
/// 在 Unix 上，路径是 `$XDG_RUNTIME_DIR/pureterm.sock`，当运行时目录不可用时，
/// 回退到 `/tmp/pureterm-{uid}.sock`。
pub fn socket_path() -> PathBuf {
    // 首先尝试 XDG_RUNTIME_DIR。
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(runtime_dir);
        if path.is_dir() {
            return path.join("pureterm.sock");
        }
    }

    // 回退：/tmp/pureterm-{uid}.sock
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/pureterm-{uid}.sock"))
}

/// PureTerm 远程控制协议的同步客户端。
///
/// 通过 Unix 域套接字连接到正在运行的 PureTerm 实例，
/// 每次连接发送一个命令，并读取 JSON 响应。
pub struct RemoteClient {
    socket: PathBuf,
    timeout: Duration,
}

impl RemoteClient {
    /// 使用默认套接字路径和 5 秒超时创建新客户端。
    pub fn new() -> Self {
        Self {
            socket: socket_path(),
            timeout: Duration::from_secs(5),
        }
    }

    /// 创建指向明确套接字路径的客户端。
    pub fn with_socket(socket: PathBuf) -> Self {
        Self {
            socket,
            timeout: Duration::from_secs(5),
        }
    }

    /// 覆盖连接/读取/写入超时。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 发送命令到正在运行的 PureTerm 实例并返回其响应。
    ///
    /// 如果服务器套接字不存在（即 PureTerm 未运行）
    /// 或连接/协议交换失败，则返回错误。
    pub fn send_command(&self, cmd: &RemoteCommand) -> Result<RemoteResponse> {
        let json = cmd.to_json()?;

        // 同步连接。
        let stream = UnixStream::connect(&self.socket).with_context(|| {
            format!(
                "failed to connect to PureTerm at {} – is the server running?",
                self.socket.display()
            )
        })?;
        stream
            .set_read_timeout(Some(self.timeout))
            .context("failed to set read timeout")?;
        stream
            .set_write_timeout(Some(self.timeout))
            .context("failed to set write timeout")?;

        let mut writer = &stream;
        // 协议：一条以 '\n' 终止的 JSON 行。
        writeln!(writer, "{json}").context("failed to write command to socket")?;
        writer.flush().context("failed to flush socket")?;

        // 读取一条 JSON 响应行。
        let reader = BufReader::new(stream);
        let line: String;
        let mut lines = reader.lines();
        match lines.next() {
            Some(Ok(l)) => line = l,
            Some(Err(e)) => bail!("failed to read response from PureTerm: {e}"),
            None => bail!("PureTerm 关闭了连接而没有发送响应"),
        }

        RemoteResponse::parse(&line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_open_tab() {
        let cmd = RemoteCommand::OpenTab {
            profile_name: Some("default".into()),
            session_type: Some("ssh".into()),
        };
        let json = cmd.to_json().unwrap();
        let parsed = RemoteCommand::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteCommand::OpenTab { .. }));
    }

    #[test]
    fn roundtrip_send_text() {
        let cmd = RemoteCommand::SendText {
            text: "ls -la\n".into(),
        };
        let json = cmd.to_json().unwrap();
        let parsed = RemoteCommand::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteCommand::SendText { .. }));
    }

    #[test]
    fn roundtrip_run_command() {
        let cmd = RemoteCommand::RunCommand {
            command: "cargo build".into(),
        };
        let json = cmd.to_json().unwrap();
        let parsed = RemoteCommand::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteCommand::RunCommand { .. }));
    }

    #[test]
    fn roundtrip_close_tab() {
        let cmd = RemoteCommand::CloseTab {
            tab_id: Some(42),
            index: None,
        };
        let json = cmd.to_json().unwrap();
        let parsed = RemoteCommand::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteCommand::CloseTab { .. }));
    }

    #[test]
    fn roundtrip_get_state() {
        let cmd = RemoteCommand::GetState;
        let json = cmd.to_json().unwrap();
        let parsed = RemoteCommand::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteCommand::GetState));
    }

    #[test]
    fn roundtrip_list_profiles() {
        let cmd = RemoteCommand::ListProfiles;
        let json = cmd.to_json().unwrap();
        let parsed = RemoteCommand::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteCommand::ListProfiles));
    }

    #[test]
    fn roundtrip_set_theme() {
        let cmd = RemoteCommand::SetTheme {
            name: "dracula".into(),
        };
        let json = cmd.to_json().unwrap();
        let parsed = RemoteCommand::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteCommand::SetTheme { .. }));
    }

    #[test]
    fn roundtrip_toggle_fullscreen() {
        let cmd = RemoteCommand::ToggleFullscreen;
        let json = cmd.to_json().unwrap();
        let parsed = RemoteCommand::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteCommand::ToggleFullscreen));
    }

    #[test]
    fn roundtrip_quit() {
        let cmd = RemoteCommand::Quit;
        let json = cmd.to_json().unwrap();
        let parsed = RemoteCommand::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteCommand::Quit));
    }

    // --- 响应往返测试 ---

    #[test]
    fn roundtrip_response_ok() {
        let resp = RemoteResponse::Ok {
            message: Some("done".into()),
        };
        let json = resp.to_json().unwrap();
        let parsed = RemoteResponse::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteResponse::Ok { .. }));
    }

    #[test]
    fn roundtrip_response_error() {
        let resp = RemoteResponse::Error {
            message: "not found".into(),
        };
        let json = resp.to_json().unwrap();
        let parsed = RemoteResponse::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteResponse::Error { .. }));
    }

    #[test]
    fn roundtrip_response_state() {
        let resp = RemoteResponse::State {
            active_tab: Some(1),
            tab_count: 3,
            tabs: vec![
                TabInfo {
                    id: 1,
                    title: "local".into(),
                    session_type: "local".into(),
                },
                TabInfo {
                    id: 2,
                    title: "ssh".into(),
                    session_type: "ssh".into(),
                },
                TabInfo {
                    id: 3,
                    title: "serial".into(),
                    session_type: "serial".into(),
                },
            ],
        };
        let json = resp.to_json().unwrap();
        let parsed = RemoteResponse::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteResponse::State { .. }));
    }

    #[test]
    fn roundtrip_response_profiles() {
        let resp = RemoteResponse::Profiles {
            profiles: vec!["default".into(), "dev".into()],
        };
        let json = resp.to_json().unwrap();
        let parsed = RemoteResponse::parse(&json).unwrap();
        assert!(matches!(parsed, RemoteResponse::Profiles { .. }));
    }

    #[test]
    fn parse_command_from_json_string() {
        let json = r#"{"type":"SendText","text":"hello"}"#;
        let cmd = RemoteCommand::parse(json).unwrap();
        assert!(matches!(cmd, RemoteCommand::SendText { .. }));
    }

    #[test]
    fn parse_response_from_json_string() {
        let json = r#"{"type":"Ok","message":"tab opened"}"#;
        let resp = RemoteResponse::parse(json).unwrap();
        assert!(matches!(resp, RemoteResponse::Ok { .. }));
    }

    #[test]
    fn send_command_fails_when_server_not_running() {
        let client = RemoteClient::with_socket(PathBuf::from("/tmp/pureterm-nonexistent-test.sock"));
        let cmd = RemoteCommand::GetState;
        let result = client.send_command(&cmd);
        assert!(result.is_err());
    }

    #[test]
    fn socket_path_is_deterministic() {
        let p1 = socket_path();
        let p2 = socket_path();
        assert_eq!(p1, p2);
    }
}
