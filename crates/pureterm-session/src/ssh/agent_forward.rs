//! SSH Agent forwarding support.

/// Manages SSH agent forwarding for a session.
///
/// Agent forwarding allows the remote host to use the local SSH agent
/// for authentication, enabling hopping between servers without copying keys.
pub struct AgentForwardManager {
    /// Whether agent forwarding has been requested.
    requested: bool,
    /// Whether the server confirmed agent forwarding.
    active: bool,
}

impl AgentForwardManager {
    pub fn new() -> Self {
        Self {
            requested: false,
            active: false,
        }
    }

    /// Request agent forwarding on a channel.
    /// The actual request is sent via `channel.request_agent_forward()`.
    pub fn request(&mut self) {
        self.requested = true;
    }

    /// Mark agent forwarding as confirmed by the server.
    pub fn confirm(&mut self) {
        self.active = true;
        log::info!("SSH agent forwarding confirmed");
    }

    /// Mark agent forwarding as denied by the server.
    pub fn deny(&mut self) {
        self.active = false;
        log::warn!("SSH agent forwarding denied by server");
    }

    pub fn is_requested(&self) -> bool {
        self.requested
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl Default for AgentForwardManager {
    fn default() -> Self {
        Self::new()
    }
}
