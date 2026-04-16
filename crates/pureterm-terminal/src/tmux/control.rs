//! Tmux control mode parser
//!
//! Parses DCS 1000p output from tmux control mode.


/// Tmux control event types
#[derive(Debug, Clone)]
pub enum ControlEvent {
    /// %session-changed <id> <name>
    SessionChanged { id: u64, name: String },
    /// %layout-change <win-id> <layout>
    LayoutChange { window_id: u64, layout: String },
    /// %output <pane-id> <data>
    Output { pane_id: u64, data: String },
    /// %exit [<reason>]
    Exit { reason: Option<String> },
    /// %window-add <win-id>
    WindowAdd { window_id: u64 },
    /// %window-close <win-id>
    WindowClose { window_id: u64 },
    /// %pane-mode <pane-id> <mode>
    PaneMode { pane_id: u64, mode: String },
    /// %begin <id>
    Begin { id: u64 },
    /// %end <id>
    End { id: u64 },
    /// %error <id> <msg>
    Error { id: u64, message: String },
    /// Unknown event
    Unknown { name: String, args: Vec<String> },
}

/// Tmux command for sending commands to tmux
#[derive(Debug, Clone)]
pub enum TmuxCommand {
    /// Select window by index
    SelectWindow { index: usize },
    /// Select pane by id
    SelectPane { id: u64 },
    /// Send keys to pane
    SendKeys { pane_id: Option<u64>, keys: String },
    /// Resize pane
    ResizePane { pane_id: Option<u64>, width: usize, height: usize },
    /// Detach client
    DetachClient { name: Option<String> },
    /// List sessions
    ListSessions,
    /// List windows
    ListWindows { session: Option<String> },
    /// List panes
    ListPanes { window: Option<String> },
    /// Custom command
    Custom(String),
}

impl TmuxCommand {
    /// Format command for tmux
    pub fn format(&self) -> String {
        match self {
            TmuxCommand::SelectWindow { index } => {
                format!("select-window -t {}", index)
            }
            TmuxCommand::SelectPane { id } => {
                format!("select-pane -t {}", id)
            }
            TmuxCommand::SendKeys { pane_id, keys } => {
                if let Some(pid) = pane_id {
                    format!("send-keys -t {} {} && send-keys Enter", pid, keys)
                } else {
                    format!("send-keys {} && send-keys Enter", keys)
                }
            }
            TmuxCommand::ResizePane { pane_id, width, height } => {
                format!("resize-pane -t {} -x {} -y {}", pane_id.unwrap_or(0), width, height)
            }
            TmuxCommand::DetachClient { name } => {
                if let Some(n) = name {
                    format!("detach-client -t {}", n)
                } else {
                    "detach-client".to_string()
                }
            }
            TmuxCommand::ListSessions => {
                "list-sessions".to_string()
            }
            TmuxCommand::ListWindows { session } => {
                if let Some(s) = session {
                    format!("list-windows -t {}", s)
                } else {
                    "list-windows".to_string()
                }
            }
            TmuxCommand::ListPanes { window } => {
                if let Some(w) = window {
                    format!("list-panes -t {}", w)
                } else {
                    "list-panes".to_string()
                }
            }
            TmuxCommand::Custom(cmd) => cmd.clone(),
        }
    }
}

/// DCS 1000p sequence wrapper
pub struct DcsSequence {
    /// The payload after DCS 1000p
    pub payload: String,
}

/// Control mode parser state
#[derive(Debug, Clone)]
pub enum ParserState {
    /// Normal, waiting for DCS start
    Normal,
    /// Seen DCS 1000p, waiting for ;
    ExpectSemicolon,
    /// Reading DCS payload until ST
    ReadingPayload,
}

/// Parser for tmux DCS 1000p sequences
pub struct ControlParser {
    /// Current parser state
    state: ParserState,
    /// Accumulated payload
    buffer: String,
    /// Pending ST bytes
    st_bytes: Vec<u8>,
}

impl ControlParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Normal,
            buffer: String::new(),
            st_bytes: Vec::new(),
        }
    }

    /// Reset parser to initial state
    pub fn reset(&mut self) {
        self.state = ParserState::Normal;
        self.buffer.clear();
        self.st_bytes.clear();
    }

    /// Process a single byte
    pub fn feed_byte(&mut self, byte: u8) -> Option<ControlEvent> {
        match self.state {
            ParserState::Normal => {
                if byte == 0x18 || byte == 0x9C {
                    // ST (string terminator) in normal mode - ignore
                    return None;
                }
                if byte == b'\x1b' {
                    // ESC
                    self.st_bytes.clear();
                    self.st_bytes.push(byte);
                    return None;
                }
                if byte == b'P' && !self.st_bytes.is_empty() && self.st_bytes[0] == b'\x1b' {
                    // DCS after ESC
                    self.st_bytes.clear();
                    self.state = ParserState::ExpectSemicolon;
                    return None;
                }
                if byte == b'\x90' {
                    // DCS in 8-bit mode
                    self.state = ParserState::ExpectSemicolon;
                    return None;
                }
                // Other byte in normal mode - could be part of a literal string
                if byte >= 0x20 && byte <= 0x7E {
                    // Printable ASCII
                    self.buffer.push(byte as char);
                    if self.buffer.len() > 10000 {
                        // Safety limit
                        self.buffer.clear();
                    }
                }
                None
            }
            ParserState::ExpectSemicolon => {
                if byte == b'0' {
                    // DCS 1000p
                    self.state = ParserState::ReadingPayload;
                    self.buffer.clear();
                    return None;
                } else {
                    // Not 1000p, go back to normal
                    self.state = ParserState::Normal;
                    None
                }
            }
            ParserState::ReadingPayload => {
                if byte == b'\\' && !self.st_bytes.is_empty() {
                    // Potential ST
                    self.st_bytes.push(byte);
                    None
                } else if !self.st_bytes.is_empty() && self.st_bytes.len() >= 2
                    && self.st_bytes[self.st_bytes.len() - 2] == b'\\'
                    && byte == b'\x9c'
                {
                    // ST (8-bit)
                    self.state = ParserState::Normal;
                    let event = self.parse_payload();
                    self.buffer.clear();
                    self.st_bytes.clear();
                    return event;
                } else if !self.st_bytes.is_empty() && self.st_bytes[self.st_bytes.len() - 1] == b'\\'
                    && byte == b'\\'
                {
                    // Escaped backslash
                    self.buffer.push(b'\\' as char);
                    self.st_bytes.clear();
                    None
                } else if byte == b'\x18' {
                    // ST (7-bit) in payload
                    self.state = ParserState::Normal;
                    let event = self.parse_payload();
                    self.buffer.clear();
                    self.st_bytes.clear();
                    return event;
                } else {
                    if !self.st_bytes.is_empty() {
                        // Was part of escape sequence but not ST
                        for &b in &self.st_bytes[1..] {
                            if b >= 0x20 && b <= 0x7E {
                                self.buffer.push(b as char);
                            }
                        }
                        self.st_bytes.clear();
                    }
                    if byte >= 0x20 && byte <= 0x7E || byte == b'\n' || byte == b'\r' {
                        self.buffer.push(byte as char);
                    }
                    None
                }
            }
        }
    }

    /// Parse the accumulated payload into a control event
    fn parse_payload(&mut self) -> Option<ControlEvent> {
        let payload = self.buffer.trim();
        if payload.is_empty() {
            return None;
        }

        // Check for %begin/%end markers (command output)
        if payload.starts_with("%begin ") {
            let id = payload[7..].parse().ok()?;
            return Some(ControlEvent::Begin { id });
        }
        if payload.starts_with("%end ") {
            let id = payload[5..].parse().ok()?;
            return Some(ControlEvent::End { id });
        }

        // Parse other %events
        if let Some(rest) = payload.strip_prefix("%session-changed ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let id = parts[0].parse().ok()?;
                return Some(ControlEvent::SessionChanged {
                    id,
                    name: parts[1].to_string(),
                });
            }
        }

        if let Some(rest) = payload.strip_prefix("%layout-change ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let win_id = parts[0].parse().ok()?;
                return Some(ControlEvent::LayoutChange {
                    window_id: win_id,
                    layout: parts[1].to_string(),
                });
            }
        }

        if let Some(rest) = payload.strip_prefix("%output ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let pane_id = parts[0].parse().ok()?;
                return Some(ControlEvent::Output {
                    pane_id,
                    data: parts[1].to_string(),
                });
            }
        }

        if payload == "%exit" {
            return Some(ControlEvent::Exit { reason: None });
        }
        if let Some(rest) = payload.strip_prefix("%exit ") {
            return Some(ControlEvent::Exit {
                reason: Some(rest.to_string()),
            });
        }

        if let Some(rest) = payload.strip_prefix("%window-add ") {
            let window_id = rest.parse().ok()?;
            return Some(ControlEvent::WindowAdd { window_id });
        }

        if let Some(rest) = payload.strip_prefix("%window-close ") {
            let window_id = rest.parse().ok()?;
            return Some(ControlEvent::WindowClose { window_id });
        }

        if let Some(rest) = payload.strip_prefix("%pane-mode ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let pane_id = parts[0].parse().ok()?;
                return Some(ControlEvent::PaneMode {
                    pane_id,
                    mode: parts[1].to_string(),
                });
            }
        }

        if let Some(rest) = payload.strip_prefix("%error ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let id = parts[0].parse().ok()?;
                return Some(ControlEvent::Error {
                    id,
                    message: parts[1].to_string(),
                });
            }
        }

        // Unknown event
        if let Some((name, args_str)) = payload.split_once(' ') {
            let args = args_str.split_whitespace().map(String::from).collect();
            Some(ControlEvent::Unknown {
                name: name.to_string(),
                args,
            })
        } else {
            Some(ControlEvent::Unknown {
                name: payload.to_string(),
                args: Vec::new(),
            })
        }
    }

    /// Process a string of data
    pub fn feed_string(&mut self, data: &str) -> Vec<ControlEvent> {
        let mut events = Vec::new();
        for byte in data.bytes() {
            if let Some(event) = self.feed_byte(byte) {
                events.push(event);
            }
        }
        events
    }

    /// Process raw tmux output (lines prefixed with %)
    /// This is a simpler interface for testing and direct tmux output
    pub fn feed_raw_string(&mut self, data: &str) -> Vec<ControlEvent> {
        let mut events = Vec::new();
        for line in data.lines() {
            let line = line.trim();
            if !line.is_empty() && line.starts_with('%') {
                if let Some(event) = self.parse_raw_event(line) {
                    events.push(event);
                }
            }
        }
        events
    }

    /// Parse a single raw tmux event line
    fn parse_raw_event(&mut self, line: &str) -> Option<ControlEvent> {
        let payload = line.strip_prefix('%')?;

        if let Some(rest) = payload.strip_prefix("session-changed ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let id = parts[0].parse().ok()?;
                return Some(ControlEvent::SessionChanged {
                    id,
                    name: parts[1].to_string(),
                });
            }
        }

        if let Some(rest) = payload.strip_prefix("layout-change ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let win_id = parts[0].parse().ok()?;
                return Some(ControlEvent::LayoutChange {
                    window_id: win_id,
                    layout: parts[1].to_string(),
                });
            }
        }

        if let Some(rest) = payload.strip_prefix("output ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let pane_id = parts[0].parse().ok()?;
                return Some(ControlEvent::Output {
                    pane_id,
                    data: parts[1].to_string(),
                });
            }
        }

        if payload == "exit" {
            return Some(ControlEvent::Exit { reason: None });
        }
        if let Some(rest) = payload.strip_prefix("exit ") {
            return Some(ControlEvent::Exit {
                reason: Some(rest.to_string()),
            });
        }

        if let Some(rest) = payload.strip_prefix("window-add ") {
            let window_id = rest.parse().ok()?;
            return Some(ControlEvent::WindowAdd { window_id });
        }

        if let Some(rest) = payload.strip_prefix("window-close ") {
            let window_id = rest.parse().ok()?;
            return Some(ControlEvent::WindowClose { window_id });
        }

        if let Some(rest) = payload.strip_prefix("pane-mode ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let pane_id = parts[0].parse().ok()?;
                return Some(ControlEvent::PaneMode {
                    pane_id,
                    mode: parts[1].to_string(),
                });
            }
        }

        if let Some(rest) = payload.strip_prefix("begin ") {
            let id = rest.parse().ok()?;
            return Some(ControlEvent::Begin { id });
        }

        if let Some(rest) = payload.strip_prefix("end ") {
            let id = rest.parse().ok()?;
            return Some(ControlEvent::End { id });
        }

        if let Some(rest) = payload.strip_prefix("error ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let id = parts[0].parse().ok()?;
                return Some(ControlEvent::Error {
                    id,
                    message: parts[1].to_string(),
                });
            }
        }

        None
    }
}

impl Default for ControlParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode a tmux command into DCS 1000p format
pub fn encode_command(cmd: &TmuxCommand) -> String {
    format!("\x1bP1000p{}\x1b\\", cmd.format())
}

/// Encode a raw string for tmux
pub fn encode_string(data: &str) -> String {
    let mut result = String::new();
    for c in data.chars() {
        match c {
            '\x1b' => result.push_str("\x1b\x5c"), // ESC \
            '\n' => result.push('\n'),
            '\r' => result.push('\r'),
            _ => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_changed() {
        let mut parser = ControlParser::new();
        let events = parser.feed_raw_string("%session-changed 1 mysession");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ControlEvent::SessionChanged { id, name } => {
                assert_eq!(*id, 1);
                assert_eq!(name, "mysession");
            }
            _ => panic!("Expected SessionChanged"),
        }
    }

    #[test]
    fn test_parse_layout_change() {
        let mut parser = ControlParser::new();
        let events = parser.feed_raw_string("%layout-change 1 80x24,0,0,80x24,0,0,0");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ControlEvent::LayoutChange { window_id, layout } => {
                assert_eq!(*window_id, 1);
                assert_eq!(layout, "80x24,0,0,80x24,0,0,0");
            }
            _ => panic!("Expected LayoutChange"),
        }
    }

    #[test]
    fn test_parse_output() {
        let mut parser = ControlParser::new();
        let events = parser.feed_raw_string("%output 1 hello");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ControlEvent::Output { pane_id, data } => {
                assert_eq!(*pane_id, 1);
                assert_eq!(data, "hello");
            }
            _ => panic!("Expected Output"),
        }
    }

    #[test]
    fn test_parse_exit() {
        let mut parser = ControlParser::new();
        let events = parser.feed_raw_string("%exit");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ControlEvent::Exit { reason: None }));

        let events2 = parser.feed_raw_string("%exit detached");
        assert_eq!(events2.len(), 1);
        match &events2[0] {
            ControlEvent::Exit { reason } => {
                assert_eq!(reason.as_ref().unwrap(), "detached");
            }
            _ => panic!("Expected Exit"),
        }
    }

    #[test]
    fn test_tmux_command_format() {
        let cmd = TmuxCommand::SelectWindow { index: 0 };
        assert_eq!(cmd.format(), "select-window -t 0");

        let cmd2 = TmuxCommand::SendKeys {
            pane_id: Some(1),
            keys: "hello".to_string(),
        };
        assert_eq!(cmd2.format(), "send-keys -t 1 hello && send-keys Enter");
    }

    #[test]
    fn test_encode_command() {
        let cmd = TmuxCommand::ListSessions;
        let encoded = encode_command(&cmd);
        assert!(encoded.starts_with("\x1bP1000p"));
        assert!(encoded.ends_with("\x1b\\"));
    }
}
