//! SSH X11 forwarding support.

use std::net::SocketAddr;

/// Configuration for X11 forwarding on an SSH channel.
#[derive(Debug, Clone)]
pub struct X11ForwardConfig {
    /// X11 authentication protocol (typically "MIT-MAGIC-COOKIE-1").
    pub auth_protocol: String,
    /// X11 authentication cookie data.
    pub auth_cookie: Vec<u8>,
    /// Screen number.
    pub screen_number: u32,
    /// Single connection mode — only one client can connect.
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

/// State of an X11 forwarding channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11State {
    /// X11 forwarding requested but not yet confirmed.
    Pending,
    /// X11 forwarding is active.
    Active,
    /// X11 forwarding was rejected by the server.
    Rejected,
    /// X11 forwarding has been closed.
    Closed,
}

/// Manages X11 forwarding for an SSH session.
pub struct X11ForwardManager {
    config: X11ForwardConfig,
    state: X11State,
    /// Local address of the X11 display (e.g., from $DISPLAY).
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

    /// Parse the DISPLAY environment variable and resolve to a socket address.
    pub fn resolve_display() -> Option<SocketAddr> {
        let display = std::env::var("DISPLAY").ok()?;
        // DISPLAY format: hostname:displaynumber.screennumber
        // or :displaynumber.screennumber (local)
        let display = display.trim_start_matches(':');
        let parts: Vec<&str> = display.split('.').collect();
        let display_num: u32 = parts.first()?.parse().ok()?;

        // TCP port = 6000 + display number
        let port = 6000 + display_num;
        Some(SocketAddr::from(([127, 0, 0, 1], port as u16)))
    }

    /// Create a manager with auto-detected display settings.
    pub fn from_env() -> Self {
        let display = Self::resolve_display();
        let mut config = X11ForwardConfig::default();
        // Generate a random auth cookie
        config.auth_cookie = random_bytes(16);
        Self {
            config,
            state: X11State::Pending,
            local_display: display,
        }
    }

    /// Build the X11 auth cookie string for the channel request.
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

// Simple random byte generation using system time
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
