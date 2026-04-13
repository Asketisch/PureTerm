use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Remote control commands that can be sent to PureTerm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RemoteCommand {
    /// Open a new tab with given profile.
    OpenTab {
        profile_name: Option<String>,
        session_type: Option<String>,
    },
    /// Close a tab by index or id.
    CloseTab {
        tab_id: Option<u64>,
        index: Option<usize>,
    },
    /// Send text to active terminal.
    SendText { text: String },
    /// Run a command in active terminal.
    RunCommand { command: String },
    /// Get current state.
    GetState,
    /// List profiles.
    ListProfiles,
    /// Set theme.
    SetTheme { name: String },
    /// Toggle fullscreen.
    ToggleFullscreen,
    /// Quit application.
    Quit,
}

impl RemoteCommand {
    /// Parse a command from a JSON string.
    pub fn parse(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("failed to parse RemoteCommand from JSON")
    }

    /// Serialize this command to a JSON string.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("failed to serialize RemoteCommand to JSON")
    }
}

/// Response from PureTerm.
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
    /// Parse a response from a JSON string.
    pub fn parse(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("failed to parse RemoteResponse from JSON")
    }

    /// Serialize this response to a JSON string.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("failed to serialize RemoteResponse to JSON")
    }
}

/// Information about a single tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: u64,
    pub title: String,
    pub session_type: String,
}

/// Returns the platform-specific IPC socket path for PureTerm.
///
/// On Unix the path is `$XDG_RUNTIME_DIR/pureterm.sock`, falling back to
/// `/tmp/pureterm-{uid}.sock` when the runtime directory is unavailable.
pub fn socket_path() -> PathBuf {
    // Try XDG_RUNTIME_DIR first.
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(runtime_dir);
        if path.is_dir() {
            return path.join("pureterm.sock");
        }
    }

    // Fallback: /tmp/pureterm-{uid}.sock
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/pureterm-{uid}.sock"))
}

/// Synchronous client for the PureTerm remote control protocol.
///
/// Connects to the running PureTerm instance over a Unix domain socket and
/// sends one command per connection, reading back the JSON response.
pub struct RemoteClient {
    socket: PathBuf,
    timeout: Duration,
}

impl RemoteClient {
    /// Create a new client using the default socket path and a 5-second timeout.
    pub fn new() -> Self {
        Self {
            socket: socket_path(),
            timeout: Duration::from_secs(5),
        }
    }

    /// Create a client pointed at an explicit socket path.
    pub fn with_socket(socket: PathBuf) -> Self {
        Self {
            socket,
            timeout: Duration::from_secs(5),
        }
    }

    /// Override the connect/read/write timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Send a command to the running PureTerm instance and return its response.
    ///
    /// Returns an error if the server socket is not present (i.e. PureTerm is
    /// not running) or if the connection / protocol exchange fails.
    pub fn send_command(&self, cmd: &RemoteCommand) -> Result<RemoteResponse> {
        let json = cmd.to_json()?;

        // Connect synchronously.
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
        // Protocol: one JSON line terminated by '\n'.
        writeln!(writer, "{json}").context("failed to write command to socket")?;
        writer.flush().context("failed to flush socket")?;

        // Read one JSON response line.
        let reader = BufReader::new(stream);
        let line: String;
        let mut lines = reader.lines();
        match lines.next() {
            Some(Ok(l)) => line = l,
            Some(Err(e)) => bail!("failed to read response from PureTerm: {e}"),
            None => bail!("PureTerm closed the connection without sending a response"),
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

    // --- Response round-trip tests ---

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
