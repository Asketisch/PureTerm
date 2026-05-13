//! Tmux control mode session.
//!
//! Spawns `tmux -C -u` and communicates via the control mode protocol.
//! Routes `%output` events through the session's broadcast channel
//! and exposes layout changes for the UI to consume.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::base::{ConnectParams, Session};

/// Events emitted by the tmux control mode session.
#[derive(Debug, Clone)]
pub enum TmuxEvent {
    /// Output from a tmux pane: (pane_id, data)
    Output { pane_id: u64, data: String },
    /// Layout changed for a window: (window_id, layout_string)
    LayoutChange { window_id: u64, layout: String },
    /// Session changed: (session_id, session_name)
    SessionChanged { id: u64, name: String },
    /// A window was added: (window_id)
    WindowAdd { window_id: u64 },
    /// A window was closed: (window_id)
    WindowClose { window_id: u64 },
    /// Pane mode changed: (pane_id, mode)
    PaneMode { pane_id: u64, mode: String },
    /// Tmux exited: (optional reason)
    Exit { reason: Option<String> },
}

/// Configuration for the tmux session.
#[derive(Debug, Clone)]
pub struct TmuxSessionConfig {
    /// Session name (for new or attach)
    pub session_name: Option<String>,
    /// Whether to attach to an existing session
    pub attach: bool,
    /// Socket path (for custom tmux socket)
    pub socket_path: Option<String>,
    /// Additional tmux flags
    pub extra_flags: Vec<String>,
}

impl Default for TmuxSessionConfig {
    fn default() -> Self {
        Self {
            session_name: None,
            attach: false,
            socket_path: None,
            extra_flags: Vec::new(),
        }
    }
}

/// Tmux control mode session implementation.
pub struct TmuxSession {
    /// The tmux child process
    child: Option<Child>,
    /// Stdin writer for sending commands to tmux
    stdin: Option<ChildStdin>,
    /// Broadcast sender for terminal output data
    output_tx: broadcast::Sender<Vec<u8>>,
    /// Broadcast sender for tmux events
    event_tx: broadcast::Sender<TmuxEvent>,
    /// Whether the session is open
    open: Arc<AtomicBool>,
    /// Command ID counter for correlating requests/responses
    next_cmd_id: u64,
    /// Current pane ID for input routing
    active_pane_id: Option<u64>,
}

impl TmuxSession {
    pub fn new(_params: ConnectParams, config: TmuxSessionConfig) -> Result<Self> {
        let (output_tx, _) = broadcast::channel(8192);
        let (event_tx, _) = broadcast::channel(256);

        let mut cmd = Command::new("tmux");

        // Socket path
        if let Some(ref socket) = config.socket_path {
            cmd.arg("-S").arg(socket);
        }

        // Control mode with UTF-8
        cmd.arg("-C").arg("-u");

        // Extra flags
        for flag in &config.extra_flags {
            cmd.arg(flag);
        }

        // New session or attach
        if config.attach {
            cmd.arg("attach-session");
            if let Some(ref name) = config.session_name {
                cmd.arg("-t").arg(name);
            }
        } else {
            cmd.arg("new-session");
            cmd.arg("-x").arg("80").arg("-y").arg("24");
            if let Some(ref name) = config.session_name {
                cmd.arg("-s").arg(name);
            }
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().context("Failed to spawn tmux")?;
        let stdin = child.stdin.take().context("Failed to get tmux stdin")?;
        let stdout = child.stdout.take().context("Failed to get tmux stdout")?;

        let open = Arc::new(AtomicBool::new(true));
        let open_clone = open.clone();
        let output_tx_clone = output_tx.clone();
        let event_tx_clone = event_tx.clone();

        // Spawn reader thread to parse tmux control mode output
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            Self::read_tmux_output(reader, &output_tx_clone, &event_tx_clone, &open_clone);
        });

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            output_tx,
            event_tx,
            open,
            next_cmd_id: 1,
            active_pane_id: None,
        })
    }

    /// Create with default config
    pub fn with_defaults() -> Result<Self> {
        let params = ConnectParams {
            session_type: crate::base::SessionType::Local {
                shell: None,
                args: vec![],
            },
            env: HashMap::new(),
            cwd: None,
        };
        Self::new(params, TmuxSessionConfig::default())
    }

    /// Parse tmux control mode output in the reader thread.
    fn read_tmux_output(
        reader: std::io::BufReader<ChildStdout>,
        output_tx: &broadcast::Sender<Vec<u8>>,
        event_tx: &broadcast::Sender<TmuxEvent>,
        open: &AtomicBool,
    ) {
        let mut lines = reader.lines();
        let mut in_command_output = false;
        let mut command_lines = Vec::new();

        while open.load(Ordering::Relaxed) {
            match lines.next() {
                Some(Ok(line)) => {
                    let trimmed = line.trim();

                    if trimmed.is_empty() {
                        continue;
                    }

                    // Handle command output boundaries
                    if trimmed.starts_with("%begin ") {
                        if let Ok(_id) = trimmed[7..].parse::<u64>() {
                            in_command_output = true;
                            command_lines.clear();
                        }
                        continue;
                    }

                    if trimmed.starts_with("%end ") {
                        if let Ok(_id) = trimmed[5..].parse::<u64>() {
                            in_command_output = false;
                            // Command output is complete — not routed to terminal
                        }
                        continue;
                    }

                    if trimmed.starts_with("%error ") {
                        in_command_output = false;
                        let rest = &trimmed[7..];
                        if let Some((id_str, msg)) = rest.split_once(' ') {
                            let _ = id_str.parse::<u64>();
                            log::warn!("Tmux command error: {}", msg);
                        }
                        continue;
                    }

                    if in_command_output {
                        command_lines.push(trimmed.to_string());
                        continue;
                    }

                    // Parse tmux notification events
                    if let Some(event) = Self::parse_event(trimmed) {
                        match &event {
                            TmuxEvent::Output { pane_id, data } => {
                                let bytes = data.as_bytes().to_vec();
                                let _ = output_tx.send(bytes);
                            }
                            TmuxEvent::Exit { .. } => {
                                let _ = event_tx.send(event);
                                open.store(false, Ordering::Relaxed);
                                return;
                            }
                            _ => {}
                        }
                        let _ = event_tx.send(event);
                    }
                }
                Some(Err(e)) => {
                    log::debug!("Tmux read error: {}", e);
                    break;
                }
                None => break,
            }
        }

        open.store(false, Ordering::Relaxed);
        let _ = event_tx.send(TmuxEvent::Exit {
            reason: Some("tmux process exited".to_string()),
        });
    }

    /// Parse a single tmux notification line into an event.
    fn parse_event(line: &str) -> Option<TmuxEvent> {
        if !line.starts_with('%') {
            return None;
        }

        if let Some(rest) = line.strip_prefix("%output ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let pane_str = parts[0].strip_prefix('%').unwrap_or(parts[0]);
                if let Ok(pane_id) = pane_str.parse::<u64>() {
                    return Some(TmuxEvent::Output {
                        pane_id,
                        data: parts[1].to_string(),
                    });
                }
            }
        }

        if let Some(rest) = line.strip_prefix("%layout-change ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                if let Ok(window_id) = parts[0].parse::<u64>() {
                    return Some(TmuxEvent::LayoutChange {
                        window_id,
                        layout: parts[1].to_string(),
                    });
                }
            }
        }

        if let Some(rest) = line.strip_prefix("%session-changed ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                if let Ok(id) = parts[0].parse::<u64>() {
                    return Some(TmuxEvent::SessionChanged {
                        id,
                        name: parts[1].to_string(),
                    });
                }
            }
        }

        if let Some(rest) = line.strip_prefix("%window-add ") {
            if let Ok(window_id) = rest.parse::<u64>() {
                return Some(TmuxEvent::WindowAdd { window_id });
            }
        }

        if let Some(rest) = line.strip_prefix("%window-close ") {
            if let Ok(window_id) = rest.parse::<u64>() {
                return Some(TmuxEvent::WindowClose { window_id });
            }
        }

        if let Some(rest) = line.strip_prefix("%pane-mode ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let pane_str = parts[0].strip_prefix('%').unwrap_or(parts[0]);
                if let Ok(pane_id) = pane_str.parse::<u64>() {
                    return Some(TmuxEvent::PaneMode {
                        pane_id,
                        mode: parts[1].to_string(),
                    });
                }
            }
        }

        if line == "%exit" {
            return Some(TmuxEvent::Exit { reason: None });
        }
        if let Some(reason) = line.strip_prefix("%exit ") {
            return Some(TmuxEvent::Exit {
                reason: Some(reason.to_string()),
            });
        }

        None
    }

    /// Subscribe to tmux events (layout changes, session changes, etc.).
    pub fn event_stream(&self) -> broadcast::Receiver<TmuxEvent> {
        self.event_tx.subscribe()
    }

    /// Send a command to tmux and get a unique command ID.
    fn send_command(&mut self, command: &str) -> u64 {
        let id = self.next_cmd_id;
        self.next_cmd_id += 1;
        if let Some(ref mut stdin) = self.stdin {
            let cmd_line = format!("{}:{}\n", id, command);
            if let Err(e) = stdin.write_all(cmd_line.as_bytes()) {
                log::warn!("Failed to write tmux command: {}", e);
            }
            let _ = stdin.flush();
        }
        id
    }

    /// Send keys to the active pane.
    pub fn send_keys(&mut self, keys: &str) {
        let pane = self.active_pane_id.unwrap_or(0);
        let cmd = format!("send-keys -t %{} {}", pane, keys);
        self.send_command(&cmd);
    }

    /// Set the active pane for input routing.
    pub fn set_active_pane(&mut self, pane_id: u64) {
        self.active_pane_id = Some(pane_id);
    }

    /// Select a tmux window.
    pub fn select_window(&mut self, index: usize) {
        self.send_command(&format!("select-window -t {}", index));
    }

    /// Resize the tmux client.
    pub fn resize_client(&mut self, width: u16, height: u16) {
        self.send_command(&format!("refresh-client -C {},{}", width, height));
    }

    /// List tmux sessions.
    pub fn list_sessions(&mut self) -> u64 {
        self.send_command("list-sessions")
    }

    /// List windows in the current session.
    pub fn list_windows(&mut self) -> u64 {
        self.send_command("list-windows")
    }

    /// List panes in the current window.
    pub fn list_panes(&mut self) -> u64 {
        self.send_command("list-panes")
    }

    /// Detach from the tmux session.
    pub fn detach(&mut self) {
        self.send_command("detach-client");
    }
}

#[async_trait]
impl Session for TmuxSession {
    fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }

    async fn start(&mut self) -> Result<()> {
        // tmux is already spawned in new(), just confirm it's open
        if self.open.load(Ordering::Relaxed) {
            Ok(())
        } else {
            Err(anyhow::anyhow!("tmux process exited immediately"))
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.resize_client(cols, rows);
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        // In tmux control mode, user input must be sent via send-keys
        // Convert raw bytes to tmux key names where possible
        let pane = self.active_pane_id.unwrap_or(0);

        if let Some(ref mut stdin) = self.stdin {
            // For raw data, use send-keys with -l flag (literal)
            // We need to escape special characters
            let escaped: String = data.iter().map(|&b| {
                match b {
                    b'\n' => "Enter".to_string(),
                    b'\t' => "Tab".to_string(),
                    b'\x7f' => "BSpace".to_string(),
                    b'\x03' => "C-c".to_string(),
                    b'\x1b' => "Escape".to_string(),
                    _ if b >= 0x20 && b < 0x7f => {
                        (b as char).to_string()
                    }
                    _ => format!("\\x{:02x}", b),
                }
            }).collect();

            // Send each key individually for special keys, or as literal for printable
            let cmd = if escaped.contains(|c: char| !c.is_ascii_graphic() && c != ' ') {
                // Has special keys — send via tmux key names
                for key in data.iter() {
                    let key_name = match *key {
                        b'\n' => "Enter".to_string(),
                        b'\t' => "Tab".to_string(),
                        b'\x7f' => "BSpace".to_string(),
                        b'\x03' => "C-c".to_string(),
                        b'\x04' => "C-d".to_string(),
                        b'\x1a' => "C-z".to_string(),
                        b'\x1b' => "Escape".to_string(),
                        b'\x01' => "C-a".to_string(),
                        b'\x05' => "C-e".to_string(),
                        b'\x17' => "C-w".to_string(),
                        b'\x15' => "C-u".to_string(),
                        b'\x0b' => "C-k".to_string(),
                        b'\x10' => "C-p".to_string(),
                        b'\x0e' => "C-n".to_string(),
                        b'\x02' => "C-b".to_string(),
                        b'\x06' => "C-f".to_string(),
                        b'\x12' => "C-r".to_string(),
                        b'\x14' => "C-t".to_string(),
                        b'\x18' => "C-x".to_string(),
                        b'\x19' => "C-y".to_string(),
                        b'\x08' => "C-h".to_string(),
                        b'\x0c' => "C-l".to_string(),
                        b if b >= 0x20 && b < 0x7f => {
                            // Printable ASCII — send as literal
                            let ch = b as char;
                            let id = self.next_cmd_id;
                            self.next_cmd_id += 1;
                            let cmd_line = format!("{}:send-keys -t %{} -l '{}'\n", id, pane, ch);
                            let _ = stdin.write_all(cmd_line.as_bytes());
                            continue;
                        }
                        b => {
                            // Other control chars — send hex escape
                            let id = self.next_cmd_id;
                            self.next_cmd_id += 1;
                            let cmd_line = format!("{}:send-keys -t %{} -l '\\x{:02x}'\n", id, pane, b);
                            let _ = stdin.write_all(cmd_line.as_bytes());
                            continue;
                        }
                    };
                    let id = self.next_cmd_id;
                    self.next_cmd_id += 1;
                    let cmd_line = format!("{}:send-keys -t %{} {}\n", id, pane, key_name);
                    let _ = stdin.write_all(cmd_line.as_bytes());
                }
                let _ = stdin.flush();
                return Ok(());
            } else {
                // All printable — send as one literal string
                let id = self.next_cmd_id;
                self.next_cmd_id += 1;
                let literal = String::from_utf8_lossy(data);
                format!("{}:send-keys -t %{} -l \"{}\"\n", id, pane, literal.replace('"', "\\\""))
            };

            let _ = stdin.write_all(cmd.as_bytes());
            let _ = stdin.flush();
        }
        Ok(())
    }

    fn kill(&mut self, _signal: &str) -> Result<()> {
        self.open.store(false, Ordering::Relaxed);
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
        }
        Ok(())
    }

    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    fn session_type_name(&self) -> &'static str {
        "tmux"
    }
}

impl Drop for TmuxSession {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Relaxed);
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output_event() {
        let event = TmuxSession::parse_event("%output %1 hello world").unwrap();
        match event {
            TmuxEvent::Output { pane_id, data } => {
                assert_eq!(pane_id, 1);
                assert_eq!(data, "hello world");
            }
            _ => panic!("Expected Output"),
        }
    }

    #[test]
    fn test_parse_layout_change() {
        let event = TmuxSession::parse_event("%layout-change @0 80x24,0,0").unwrap();
        match event {
            TmuxEvent::LayoutChange { window_id, layout } => {
                assert_eq!(window_id, 0);
                assert_eq!(layout, "80x24,0,0");
            }
            _ => panic!("Expected LayoutChange"),
        }
    }

    #[test]
    fn test_parse_session_changed() {
        let event = TmuxSession::parse_event("%session-changed 1 mysession").unwrap();
        match event {
            TmuxEvent::SessionChanged { id, name } => {
                assert_eq!(id, 1);
                assert_eq!(name, "mysession");
            }
            _ => panic!("Expected SessionChanged"),
        }
    }

    #[test]
    fn test_parse_exit() {
        let event = TmuxSession::parse_event("%exit").unwrap();
        assert!(matches!(event, TmuxEvent::Exit { reason: None }));

        let event2 = TmuxSession::parse_event("%exit detached").unwrap();
        assert!(matches!(event2, TmuxEvent::Exit { reason: Some(r) } if r == "detached"));
    }

    #[test]
    fn test_parse_window_events() {
        let event = TmuxSession::parse_event("%window-add @1").unwrap();
        assert!(matches!(event, TmuxEvent::WindowAdd { window_id: 1 }));

        let event = TmuxSession::parse_event("%window-close @1").unwrap();
        assert!(matches!(event, TmuxEvent::WindowClose { window_id: 1 }));
    }

    #[test]
    fn test_parse_pane_mode() {
        let event = TmuxSession::parse_event("%pane-mode %0 copy-mode").unwrap();
        match event {
            TmuxEvent::PaneMode { pane_id, mode } => {
                assert_eq!(pane_id, 0);
                assert_eq!(mode, "copy-mode");
            }
            _ => panic!("Expected PaneMode"),
        }
    }

    #[test]
    fn test_unknown_line_returns_none() {
        assert!(TmuxSession::parse_event("regular text").is_none());
        assert!(TmuxSession::parse_event("").is_none());
        assert!(TmuxSession::parse_event("%unknown-event foo").is_none());
    }
}
