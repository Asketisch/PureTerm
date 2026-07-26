//! SSH 子命令组 — profile/连接/隧道。Unix only。
//!
//! connect 带 `[user@]host` 走 daemon-direct（GAP-5.1 M3），`--index` 走
//! GUI-IPC；tunnel/tunnel-stop/tunnel-list 走 daemon SOCKS5（GAP-5.2）。

use clap::Subcommand;
use serde_json::{json, Value};

#[cfg(unix)]
#[derive(Subcommand)]
pub enum SshSub {
    /// List configured SSH profiles
    ListProfiles,
    /// Connect to an SSH profile by index
    Connect {
        /// `[user@]host` — daemon-direct: opens a persistent puretermd session
        /// and attaches the current terminal (GAP-5.1 M3 DoD). Omit and pass
        /// `--index` instead to ask the running GUI to open a tab.
        target: Option<String>,
        /// Profile index for the GUI-IPC path (opens a tab in the running GUI).
        #[arg(long)]
        index: Option<usize>,
        /// SSH port (daemon-direct path only).
        #[arg(long)]
        port: Option<u16>,
        /// Identity file / private key path (daemon-direct path only).
        #[arg(long)]
        identity: Option<String>,
        /// GAP-5.4: forward the local ssh-agent to the remote host
        /// (`-o ForwardAgent=yes`). Default off (trust risk on untrusted hosts).
        #[arg(short = 'A', long)]
        forward_agent: bool,
        /// GAP-40: local port forward (`ssh -L`), repeatable (`8080:localhost:80`).
        #[arg(short = 'L', long = "local-forward")]
        local_forwards: Vec<String>,
        /// GAP-40: remote port forward (`ssh -R`), repeatable.
        #[arg(short = 'R', long = "remote-forward")]
        remote_forwards: Vec<String>,
        /// GAP-40: untrusted X11 forwarding (`ssh -X`).
        #[arg(short = 'X')]
        forward_x11: bool,
        /// GAP-40: trusted X11 forwarding (`ssh -Y`).
        #[arg(short = 'Y')]
        forward_x11_trusted: bool,
    },
    /// Disconnect (close the SSH tab). GAP-5.6: `--pane-id` targets a specific
    /// pane; omit to disconnect the active pane.
    Disconnect {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// GAP-5.6: query every SSH pane's connection state (`ssh.state` Socket API).
    /// Shows connected / reconnecting / failed (+ typed failure kind) per pane.
    State,
    /// GAP-5.2: open a russh-backed SOCKS5 tunnel through a gateway (daemon-owned).
    /// `curl --socks5-hostname 127.0.0.1:<local-port> https://...` then routes via
    /// the gateway, reaching hosts only it can see.
    Tunnel {
        /// `[user@]host` gateway.
        target: String,
        /// SSH port (default 22).
        #[arg(long)]
        port: Option<u16>,
        /// Identity file / private key path (publickey auth).
        #[arg(short = 'i', long)]
        identity: Option<String>,
        /// Local SOCKS5 listen port (default 1080).
        #[arg(long)]
        local_port: Option<u16>,
        /// Password (password auth, or private-key passphrase when used with
        /// `--identity`). If omitted and no `--identity`, prompted on stdin.
        #[arg(long)]
        password: Option<String>,
        /// Resolve DNS locally instead of via the gateway (default: gateway
        /// resolves, i.e. socks5h / `--socks5-hostname`).
        #[arg(long)]
        no_remote_dns: bool,
        /// GAP-5.4: forward the local ssh-agent over the tunnel's session and
        /// prefer ssh-agent keys for auth (no `--identity` needed). Default off.
        #[arg(short = 'A', long)]
        forward_agent: bool,
    },
    /// GAP-5.2: stop a tunnel by id.
    TunnelStop {
        /// Tunnel id from `ssh tunnel-list`.
        id: String,
    },
    /// GAP-5.2: list active tunnels.
    TunnelList,
}

#[cfg(unix)]
impl SshSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            SshSub::ListProfiles => ("ssh.list_profiles", None),
            SshSub::Connect { index: Some(i), .. } => {
                ("ssh.connect", Some(json!({ "index": i })))
            }
            SshSub::Connect { .. } => ("ssh.connect", None),
            SshSub::Disconnect { pane_id } => {
                ("ssh.disconnect", pane_id.map(|p| json!({ "pane_id": p })))
            }
            SshSub::State => ("ssh.state", None),
            // GAP-5.2: daemon-direct (handled in `run` before the GUI-IPC fallback).
            // These arms are unreachable from the IPC path but keep the match total.
            SshSub::Tunnel { .. } => ("tunnel.start", None),
            SshSub::TunnelStop { .. } => ("tunnel.stop", None),
            SshSub::TunnelList => ("tunnel.list", None),
        }
    }
}
