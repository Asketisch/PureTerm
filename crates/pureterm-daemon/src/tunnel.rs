//! GAP-5.2: in-process SSH SOCKS5 tunnel, daemon-owned and russh-backed.
//!
//! Unlike the PTY SSH path (which spawns an `ssh -tt` child + ControlMaster
//! socket), the tunnel is a real `russh::client` session owned by `puretermd`.
//! Each SOCKS5 CONNECT becomes a `channel_open_direct_tcpip`, giving us a
//! Rust-level channel object that future agent-browser (GAP-7.8) work can drive
//! directly. The tunnel survives GUI restarts because it lives in the daemon.
//!
//! Auth: `identity_file` (publickey) or `password` (password auth). If neither
//! is supplied, the tunnel enters `NeedAuth` and waits for `tunnel.provide_auth`.
//!
//! SOCKS5 is hand-written here (no `fast-socks5` dependency): no-auth +
//! CONNECT-only. `atyp=3` (domain) targets are forwarded literally so the SSH
//! server resolves them remotely — this is what `curl --socks5-hostname` needs.
//!
//! `connect`/authenticate are async, but the control-plane `dispatch` is sync.
//! So `start` binds the listener + registers the tunnel synchronously, then
//! spawns the async connect task on the daemon's multi-thread runtime and
//! returns the tunnel id immediately. Status is polled via `list`.
//!
//! 本模块按职责拆分为子模块 (见 `tunnel/`):
//! - [`manager`]  — `TunnelManager` 实现 (生命周期 / 持久化 / host-key store)。
//! - [`handler`]  — russh `Handler` 实现 + SOCKS5 服务 + 认证。
//! - [`tests`]    — 单测。
//!
//! 根文件只保留:模块文档、SOCKS5 协议常量、tunnel id 生成、状态机
//! (`TunnelLive`)、可序列化描述 (`TunnelInfo`)、内部条目 (`TunnelEntry`)、
//! `TunnelManager` 注册表 struct。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use crate::known_hosts::KnownHosts;

mod handler;
mod manager;
#[cfg(test)]
mod tests;

/// SOCKS5 protocol version byte.
pub(super) const SOCKS5: u8 = 5;

/// Fresh tunnel id (mirrors `session::new_session_id`): `<pid>-<counter>`.
pub(super) fn new_tunnel_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tun-{}-{}", std::process::id(), n)
}

/// Live tunnel state machine.
#[derive(Clone, Debug)]
pub(super) enum TunnelLive {
    Connecting,
    Connected,
    NeedAuth,
    Failed(String),
    /// GAP-5.2: a config restored from `tunnels.json` at daemon startup but not
    /// yet (re)started. Config-only restore: no listener bound, no task.
    Stopped,
}

impl TunnelLive {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            TunnelLive::Connecting => "connecting",
            TunnelLive::Connected => "connected",
            TunnelLive::NeedAuth => "need_auth",
            TunnelLive::Failed(_) => "failed",
            TunnelLive::Stopped => "stopped",
        }
    }
}

/// Serializable tunnel descriptor (for `tunnel.list`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TunnelInfo {
    pub id: String,
    pub host: String,
    pub user: Option<String>,
    pub port: u16,
    /// Bound local SOCKS5 address (`127.0.0.1:<port>`).
    pub local_addr: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A live tunnel entry. The async connect/copy task updates `status`; the
/// control plane reads it. `auth_tx` is set when the task parks in `NeedAuth`
/// so `provide_auth` can hand it the secret.
pub(super) struct TunnelEntry {
    pub(super) host: String,
    pub(super) user: Option<String>,
    pub(super) port: u16,
    pub(super) local_addr: String,
    /// Bound local SOCKS5 port (carried so a live entry can be snapshotted to a
    /// `TunnelConfig` for persistence, and a restored `Stopped` entry shows it).
    pub(super) local_port: u16,
    pub(super) remote_dns: bool,
    pub(super) forward_agent: bool,
    #[allow(dead_code)]
    pub(super) identity_file: Option<String>,
    pub(super) status: TunnelLive,
    pub(super) auth_tx: Option<oneshot::Sender<String>>,
    pub(super) abort: Option<tokio::task::AbortHandle>,
}

/// Central tunnel registry, shared between the (sync) control plane and the
/// (async) connect tasks. Mirrors `session::SessionManager`'s shape.
#[derive(Clone, Default)]
pub struct TunnelManager {
    pub(super) tunnels: Arc<Mutex<HashMap<String, Arc<Mutex<TunnelEntry>>>>>,
    /// GAP-5.2 TOFU host-key store, set once at daemon startup. Cloned into
    /// each tunnel's `ClientHandler`. `OnceLock` keeps `TunnelManager: Default`
    /// (unset in unit tests) while letting `runtime` set it after construction.
    pub(super) known_hosts: Arc<std::sync::OnceLock<Arc<KnownHosts>>>,
}
