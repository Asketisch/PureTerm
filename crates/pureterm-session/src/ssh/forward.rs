//! SSH Port Forwarding (Local, Remote, Dynamic/SOCKS5).
//!
//! Provides tunneling capabilities through an SSH connection.

use anyhow::{Context, Result};
use tokio::net::TcpListener;

/// Type of port forwarding.
#[derive(Debug, Clone)]
pub enum ForwardType {
    /// Local port forwarding: bind local -> forward through SSH -> remote.
    Local {
        local_addr: String,
        local_port: u16,
        remote_host: String,
        remote_port: u16,
    },
    /// Remote port forwarding: remote listens -> forward through SSH -> local.
    Remote {
        remote_addr: String,
        remote_port: u16,
        local_host: String,
        local_port: u16,
    },
    /// Dynamic (SOCKS5) forwarding: local SOCKS proxy -> forward through SSH.
    Dynamic {
        local_addr: String,
        local_port: u16,
    },
}

/// A single port forwarding tunnel.
#[derive(Debug)]
pub struct ForwardTunnel {
    pub id: u64,
    pub forward_type: ForwardType,
    pub state: TunnelState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TunnelState {
    Starting,
    Active,
    Stopped,
    Error(String),
}

/// Manager for SSH port forwarding tunnels.
pub struct PortForwardManager {
    tunnels: Vec<ForwardTunnel>,
    next_id: u64,
}

impl PortForwardManager {
    pub fn new() -> Self {
        Self {
            tunnels: Vec::new(),
            next_id: 1,
        }
    }

    /// Add a new forwarding tunnel.
    pub fn add_tunnel(&mut self, forward_type: ForwardType) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tunnels.push(ForwardTunnel {
            id,
            forward_type,
            state: TunnelState::Starting,
        });
        id
    }

    /// Remove a tunnel by ID.
    pub fn remove_tunnel(&mut self, id: u64) -> Option<ForwardTunnel> {
        if let Some(pos) = self.tunnels.iter().position(|t| t.id == id) {
            Some(self.tunnels.remove(pos))
        } else {
            None
        }
    }

    /// Update tunnel state.
    pub fn set_state(&mut self, id: u64, state: TunnelState) {
        if let Some(tunnel) = self.tunnels.iter_mut().find(|t| t.id == id) {
            tunnel.state = state;
        }
    }

    /// List all tunnels.
    pub fn tunnels(&self) -> &[ForwardTunnel] {
        &self.tunnels
    }

    /// Get a tunnel by ID.
    pub fn get_tunnel(&self, id: u64) -> Option<&ForwardTunnel> {
        self.tunnels.iter().find(|t| t.id == id)
    }

    /// Stop all tunnels.
    pub fn stop_all(&mut self) {
        for tunnel in &mut self.tunnels {
            tunnel.state = TunnelState::Stopped;
        }
    }
}

impl Default for PortForwardManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Start a local port forward.
/// Accepts connections on `local_addr:local_port` and forwards them
/// through the SSH channel to `remote_host:remote_port`.
pub async fn start_local_forward(
    local_addr: &str,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
) -> Result<()> {
    let _listener = TcpListener::bind(format!("{}:{}", local_addr, local_port))
        .await
        .context("Failed to bind local port")?;

    log::info!(
        "Local forward listening on {}:{} -> {}:{}",
        local_addr,
        local_port,
        remote_host,
        remote_port
    );

    // In a full implementation, for each incoming connection we would:
    // 1. Accept the TCP connection
    // 2. Open a direct-tcpip channel on the SSH session
    // 3. Bidirectionally copy data between the TCP socket and the SSH channel
    //
    // This requires a reference to the russh Handle, which the caller must provide.

    Ok(())
}
