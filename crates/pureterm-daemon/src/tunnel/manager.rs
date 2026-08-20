//! `TunnelManager` 实现:隧道生命周期 (start / stop / provide_auth)、
//! 持久化快照 (snapshot_configs / register_stopped)、TOFU host-key store 注入。
//! 异步连接 / SOCKS5 服务委托给 [`super::handler`] 模块。

use std::net::TcpListener as StdTcpListener;
use std::sync::{Arc, Mutex};

use crate::known_hosts::KnownHosts;

use super::handler::run_tunnel;
use super::{new_tunnel_id, TunnelEntry, TunnelInfo, TunnelLive, TunnelManager};

impl TunnelManager {
    /// 注册 + 启动一条隧道。同步绑定 SOCKS5 监听器,使
    /// 返回的 id 立即可用(端口在异步连接开始前就已保留)。
    /// 返回隧道 id。
    pub fn start(
        &self,
        rt: &tokio::runtime::Handle,
        host: String,
        user: Option<String>,
        port: u16,
        identity_file: Option<String>,
        password: Option<String>,
        local_port: u16,
        remote_dns: bool,
        forward_agent: bool,
    ) -> Result<String, String> {
        // 先绑定,使端口冲突快速失败(在触碰网络之前)。
        let listener = StdTcpListener::bind(("127.0.0.1", local_port))
            .map_err(|e| format!("bind 127.0.0.1:{local_port} failed: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("set_nonblocking failed: {e}"))?;
        let local_addr = listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| format!("127.0.0.1:{local_port}"));

        let id = new_tunnel_id();
        let entry = Arc::new(Mutex::new(TunnelEntry {
            host: host.clone(),
            user: user.clone(),
            port,
            local_addr: local_addr.clone(),
            local_port,
            remote_dns,
            forward_agent,
            identity_file: identity_file.clone(),
            status: TunnelLive::Connecting,
            auth_tx: None,
            abort: None,
        }));

        let entry_for_task = entry.clone();
        let known_hosts = self.known_hosts.get().cloned();
        let join = rt.spawn(run_tunnel(
            entry_for_task,
            listener,
            host,
            user,
            port,
            identity_file,
            password,
            remote_dns,
            forward_agent,
            known_hosts,
        ));
        entry.lock().unwrap().abort = Some(join.abort_handle());

        self.tunnels.lock().unwrap().insert(id.clone(), entry);
        Ok(id)
    }

    /// 把存活隧道枚举为可序列化描述符。
    pub fn list(&self) -> Vec<TunnelInfo> {
        self.tunnels
            .lock()
            .unwrap()
            .iter()
            .map(|(id, e)| {
                let e = e.lock().unwrap();
                let error = match &e.status {
                    TunnelLive::Failed(reason) => Some(reason.clone()),
                    _ => None,
                };
                TunnelInfo {
                    id: id.clone(),
                    host: e.host.clone(),
                    user: e.user.clone(),
                    port: e.port,
                    local_addr: e.local_addr.clone(),
                    status: e.status.as_str().to_string(),
                    error,
                }
            })
            .collect()
    }

    /// 停止 + 移除隧道(中止连接/accept 任务;丢弃 russh
    /// `Handle` 即断开 SSH 会话)。返回它是否曾存在。
    pub fn stop(&self, id: &str) -> bool {
        if let Some(entry) = self.tunnels.lock().unwrap().remove(id) {
            let mut e = entry.lock().unwrap();
            if let Some(abort) = e.abort.take() {
                abort.abort();
            }
            true
        } else {
            false
        }
    }

    /// 把认证机密交给驻留在 `NeedAuth` 的隧道。隧道未知
    /// 或当前不在等待认证(如仍在连接中,
    /// 或已认证)时报错。
    pub fn provide_auth(&self, id: &str, secret: String) -> Result<(), String> {
        let entry = self
            .tunnels
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("unknown tunnel {id}"))?;
        let mut e = entry.lock().unwrap();
        let tx = e
            .auth_tx
            .take()
            .ok_or_else(|| "tunnel is not awaiting auth".to_string())?;
        tx.send(secret).map_err(|_| "auth receiver dropped".to_string())
    }

    /// 设置共享的 TOFU 主机密钥存储(守护进程启动时由 `runtime`
    /// 调用一次,先于任何隧道启动)。后续隧道在
    /// `ClientHandler::check_server_key`.
    pub fn set_known_hosts(&self, kh: Arc<KnownHosts>) {
        let _ = self.known_hosts.set(kh);
    }

    /// 把所有条目(存活 + 恢复的已停止)快照为可序列化配置,
    /// 供 `tunnels.json` 持久化。
    pub fn snapshot_configs(&self) -> Vec<crate::persist::TunnelConfig> {
        self.tunnels
            .lock()
            .unwrap()
            .values()
            .map(|e| {
                let e = e.lock().unwrap();
                crate::persist::TunnelConfig {
                    host: e.host.clone(),
                    user: e.user.clone(),
                    port: e.port,
                    identity_file: e.identity_file.clone(),
                    local_port: e.local_port,
                    remote_dns: e.remote_dns,
                    forward_agent: e.forward_agent,
                }
            })
            .collect()
    }

    /// GAP-5.2 仅配置恢复:把持久化配置注册为 `Stopped` 条目
    /// (在 `tunnel.list` 中可见),不绑监听器、无任务。用户
    /// 经 `tunnel.start` 显式重启它。
    pub fn register_stopped(&self, cfg: crate::persist::TunnelConfig) {
        let id = new_tunnel_id();
        let local_port = cfg.local_port;
        let entry = Arc::new(Mutex::new(TunnelEntry {
            host: cfg.host,
            user: cfg.user,
            port: cfg.port,
            local_addr: format!("127.0.0.1:{local_port}"),
            local_port,
            remote_dns: cfg.remote_dns,
            forward_agent: cfg.forward_agent,
            identity_file: cfg.identity_file,
            status: TunnelLive::Stopped,
            auth_tx: None,
            abort: None,
        }));
        self.tunnels.lock().unwrap().insert(id, entry);
    }
}
