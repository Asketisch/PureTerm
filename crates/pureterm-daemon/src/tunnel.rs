//! GAP-5.2:进程内 SSH SOCKS5 隧道,由守护进程持有、russh 支撑。
//!
//! 与 PTY SSH 路径(拉起 `ssh -tt` 子进程 + ControlMaster
//! socket)不同,隧道是一个由 `puretermd` 持有的真实 `russh::client` 会话。
//! 每个 SOCKS5 CONNECT 成为一个 `channel_open_direct_tcpip`,给我们
//! 一个 Rust 级的 channel 对象,未来的 agent 浏览器(GAP-7.8)工作可以直接
//! 驱动它。隧道在 GUI 重启后存活,因为它活在守护进程里。
//!
//! 认证:`identity_file`(publickey)或 `password`(密码认证)。两者
//! 都未提供时,隧道进入 `NeedAuth` 等待 `tunnel.provide_auth`。
//!
//! SOCKS5 在此手写(不依赖 `fast-socks5`):免认证 +
//! 仅 CONNECT。`atyp=3`(域名)目标按字面转发,由 SSH
//! 服务器在远端解析——这正是 `curl --socks5-hostname` 所需。
//!
//! `connect`/认证是异步的,但控制面的 `dispatch` 是同步的。
//! 因此 `start` 同步绑定监听器 + 注册隧道,然后
//! 在守护进程的多线程 runtime 上派发异步连接任务,并
//! 立即返回隧道 id。状态经 `list` 轮询。
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

/// SOCKS5 协议版本字节。
pub(super) const SOCKS5: u8 = 5;

/// 新隧道 id(镜像 `session::new_session_id`):`<pid>-<counter>`。
pub(super) fn new_tunnel_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tun-{}-{}", std::process::id(), n)
}

/// 存活隧道状态机。
#[derive(Clone, Debug)]
pub(super) enum TunnelLive {
    Connecting,
    Connected,
    NeedAuth,
    Failed(String),
    /// GAP-5.2:守护进程启动时从 `tunnels.json` 恢复但尚未
    /// (重新)启动的配置。仅恢复配置:不绑监听器、无任务。
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

/// 可序列化的隧道描述符(供 `tunnel.list`)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct TunnelInfo {
    pub id: String,
    pub host: String,
    pub user: Option<String>,
    pub port: u16,
    /// 已绑定的本地 SOCKS5 地址(`127.0.0.1:<port>`)。
    pub local_addr: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 一个存活隧道条目。异步连接/拷贝任务更新 `status`;
/// 控制面读取它。任务驻留在 `NeedAuth` 时设置 `auth_tx`,
/// 使 `provide_auth` 能把机密交给它。
pub(super) struct TunnelEntry {
    pub(super) host: String,
    pub(super) user: Option<String>,
    pub(super) port: u16,
    pub(super) local_addr: String,
    /// 已绑定的本地 SOCKS5 端口(携带它,使存活条目可快照为
    /// `TunnelConfig` 供持久化,恢复的 `Stopped` 条目也能展示)。
    pub(super) local_port: u16,
    pub(super) remote_dns: bool,
    pub(super) forward_agent: bool,
    #[allow(dead_code)]
    pub(super) identity_file: Option<String>,
    pub(super) status: TunnelLive,
    pub(super) auth_tx: Option<oneshot::Sender<String>>,
    pub(super) abort: Option<tokio::task::AbortHandle>,
}

/// 隧道中心注册表,由(同步的)控制面与
/// (异步的)连接任务共享。镜像 `session::SessionManager` 的形状。
#[derive(Clone, Default)]
pub struct TunnelManager {
    pub(super) tunnels: Arc<Mutex<HashMap<String, Arc<Mutex<TunnelEntry>>>>>,
    /// GAP-5.2 TOFU 主机密钥存储,守护进程启动时设置一次。克隆进
    /// 每条隧道的 `ClientHandler`。`OnceLock` 保持 `TunnelManager: Default`
    /// (单元测试中不设置),同时允许 `runtime` 在构造后设置它。
    pub(super) known_hosts: Arc<std::sync::OnceLock<Arc<KnownHosts>>>,
}
