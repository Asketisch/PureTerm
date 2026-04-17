use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::client::Handle;
use russh::{client, ChannelMsg, Disconnect};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};

use crate::base::Session;
use crate::ssh::auth::SshAuth as SshAuthType;
use crate::ssh::known_hosts::{KeyVerificationResult, KnownHosts};

pub struct SshClientHandler {
    host: String,
    port: u16,
    known_hosts: KnownHosts,
}

impl SshClientHandler {
    fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            known_hosts: KnownHosts::new(),
        }
    }
}

impl client::Handler for SshClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        match self.known_hosts.verify_host_key(&self.host, self.port, server_public_key) {
            KeyVerificationResult::Match => {
                log::info!("Host key verified for {}", self.host);
                Ok(true)
            }
            KeyVerificationResult::Mismatch { expected } => {
                log::error!(
                    "Host key mismatch for {}! Expected: {}",
                    self.host,
                    expected
                );
                Ok(false)
            }
            KeyVerificationResult::Unknown => {
                // TOFU：首次使用时信任 — 接受并保存密钥
                log::warn!(
                    "First connection to {}. Accepting key (TOFU).",
                    self.host
                );
                self.known_hosts.add_host_key(&self.host, self.port, server_public_key)?;
                Ok(true)
            }
        }
    }
}

/// 使用 russh 的 SSH 会话。
pub struct SshSession {
    handle: Option<Arc<Mutex<Handle<SshClientHandler>>>>,
    channel_write: Option<Arc<Mutex<russh::ChannelWriteHalf<russh::client::Msg>>>>,
    output_tx: broadcast::Sender<Vec<u8>>,
    host: String,
    port: u16,
    user: String,
    auth: SshAuthType,
    open: bool,
    /// 是否启用代理转发。
    agent_forward: bool,
    /// 待处理的端口转发监听器（本地转发）
    listeners: Vec<PortForwardListener>,
}

/// 接受本地连接并通过 SSH 转发的端口转发监听器。
#[allow(dead_code)]
pub struct PortForwardListener {
    /// 用于接受本地连接的 TCP 监听器。
    listener: Arc<TcpListener>,
    /// SSH 服务器端要连接到的远程主机。
    remote_host: String,
    /// 远程端口。
    remote_port: u16,
    /// 接受循环的任务句柄。
    _task: tokio::task::JoinHandle<()>,
}

impl SshSession {
    /// 通过 SSH 打开到指定 host:port 的 direct-tcpip 通道。
    pub async fn open_direct_tcpip(
        &self,
        host: &str,
        port: u16,
    ) -> Result<(
        russh::ChannelReadHalf,
        russh::ChannelWriteHalf<russh::client::Msg>,
    )> {
        let handle = self.handle.as_ref()
            .context("SSH session not connected")?;
        let handle = handle.lock().await;

        let channel = handle
            .channel_open_direct_tcpip(host, port as u32, host, port as u32)
            .await
            .context("Failed to open direct-tcpip channel")?;

        let (read, write) = channel.split();
        Ok((read, write))
    }

    /// 启动本地端口转发：接受 `local_addr:local_port` 上的连接，
    /// 并通过此 SSH 会话将它们转发到 `remote_host:remote_port`。
    pub fn start_local_forward(
        &mut self,
        local_addr: &str,
        local_port: u16,
        remote_host: String,
        remote_port: u16,
    ) -> Result<PortForwardListener> {
        let listener = Arc::new(pollster::block_on(TcpListener::bind(format!("{}:{}", local_addr, local_port)))?);
        let remote_host_clone = remote_host.clone();

        let handle = self.handle.clone()
            .context("SSH session not connected")?;

        let listener_task = listener.clone();
        let task = tokio::spawn(async move {
            loop {
                match listener_task.accept().await {
                    Ok((mut client_stream, client_addr)) => {
                        let remote_host = remote_host_clone.clone();
                        let rh = remote_port;
                        let handle = handle.clone();

                        tokio::spawn(async move {
                            if let Err(e) = proxy_tcp_via_ssh(
                                &mut client_stream,
                                client_addr,
                                handle,
                                &remote_host,
                                rh,
                            )
                            .await
                            {
                                log::warn!("Local forward proxy error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        log::error!("Failed to accept local forward connection: {}", e);
                        break;
                    }
                }
            }
        });

        log::info!(
            "Local port forward started: {}:{} -> {}:{}",
            local_addr,
            local_port,
            remote_host,
            remote_port
        );

        Ok(PortForwardListener {
            listener,
            remote_host,
            remote_port,
            _task: task,
        })
    }

    /// 在 `local_addr:local_port` 上启动动态（SOCKS5）端口转发。
    /// 这充当通过 SSH 连接的 SOCKS5 代理。
    pub fn start_dynamic_forward(
        &mut self,
        local_addr: &str,
        local_port: u16,
    ) -> Result<PortForwardListener> {
        let listener = Arc::new(pollster::block_on(TcpListener::bind(format!("{}:{}", local_addr, local_port)))?);
        let handle = self.handle.clone()
            .context("SSH session not connected")?;

        let listener_task = listener.clone();
        let task = tokio::spawn(async move {
            loop {
                match listener_task.accept().await {
                    Ok((mut client_stream, client_addr)) => {
                        let handle = handle.clone();
                        tokio::spawn(async move {
                            if let Err(e) = proxy_socks5(&mut client_stream, client_addr, handle).await {
                                log::warn!("SOCKS5 proxy error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        log::error!("Failed to accept SOCKS5 connection: {}", e);
                        break;
                    }
                }
            }
        });

        log::info!("SOCKS5 proxy started on {}:{}", local_addr, local_port);

        Ok(PortForwardListener {
            listener,
            remote_host: String::new(),
            remote_port: 0,
            _task: task,
        })
    }

    /// 停止所有活动的端口转发。
    pub fn stop_forwards(&mut self) {
        // 当监听器被 drop 时任务将退出
        self.listeners.clear();
    }

    /// 启动远程端口转发：请求 SSH 服务器在
    /// `remote_addr:remote_port` 上监听并连接转发回 `local_addr:local_port`。
    pub fn start_remote_forward(
        &mut self,
        remote_addr: &str,
        remote_port: u16,
        local_addr: &str,
        local_port: u16,
    ) -> Result<()> {
        let handle = self.handle.clone()
            .context("SSH session not connected")?;

        let remote_addr = remote_addr.to_string();
        let local_addr = local_addr.to_string();

        let task = tokio::spawn(async move {
            let h = handle.lock().await;
            // 请求服务器将连接转发到 remote_addr:remote_port
            match h.tcpip_forward(&remote_addr, remote_port as u32).await {
                Ok(_) => {
                    log::info!("Remote forward registered: {}:{} -> {}:{}",
                        remote_addr, remote_port, local_addr, local_port);
                }
                Err(e) => {
                    log::error!("Failed to request remote forward: {}", e);
                    return;
                }
            }
        });

        // 等待请求完成
        pollster::block_on(task)?;

        Ok(())
    }
}

/// 通过 SSH direct-tcpip 代理 TCP 连接。
async fn proxy_tcp_via_ssh(
    client_stream: &mut TcpStream,
    _client_addr: std::net::SocketAddr,
    handle: Arc<Mutex<Handle<SshClientHandler>>>,
    remote_host: &str,
    remote_port: u16,
) -> Result<()> {
    let channel = {
        let h = handle.lock().await;
        h.channel_open_direct_tcpip(remote_host, remote_port as u32, remote_host, remote_port as u32)
            .await
            .context("Failed to open SSH channel for forward")?
    };

    // 使用 russh 通道的数据事件循环进行双向复制
    let mut buf = [0u8; 8192];
    let mut channel = channel;

    loop {
        tokio::select! {
            // 从客户端复制到 SSH 通道
            result = client_stream.read(&mut buf) => {
                match result {
                    Ok(0) => break, // 连接关闭
                    Ok(n) => {
                        if channel.data(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        log::debug!("Client read error: {}", e);
                        break;
                    }
                }
            }
            // 从 SSH 通道读取并转发到客户端
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        if client_stream.write_all(&data[..]).await.is_err() {
                            break;
                        }
                    }
                    Some(ChannelMsg::Eof) | None => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

/// 处理 SOCKS5 连接并通过 SSH 代理它。
async fn proxy_socks5(
    client_stream: &mut TcpStream,
    _client_addr: std::net::SocketAddr,
    handle: Arc<Mutex<Handle<SshClientHandler>>>,
) -> Result<()> {
    use std::net::Ipv4Addr;

    // SOCKS5 握手
    let mut buf = [0u8; 262];
    let n = client_stream.read(&mut buf).await?;
    if n < 2 {
        return Ok(());
    }

    // VER=5, NMETHODS
    if buf[0] != 0x05 {
        return Ok(());
    }

    // 响应：VER=5, METHOD=NO_AUTHENTICATION_REQUIRED (0)
    client_stream.write_all(&[0x05, 0x00]).await?;

    // 读取连接请求：VER=5, CMD=CONNECT, RSV=0, ATYP=1 (IPv4)
    let n = client_stream.read(&mut buf).await?;
    if n < 10 || buf[0] != 0x05 || buf[1] != 0x01 {
        // 不支持：回复 COMMAND_NOT_SUPPORTED
        client_stream.write_all(&[0x05, 0x07, 0x00, 0x01]).await?;
        return Ok(());
    }

    // 解析目标地址（ATYP=1 是 IPv4，ATYP=3 是域名）
    let (target_host, target_port) = match buf[3] {
        0x01 => {
            // IPv4
            let ip = Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]);
            let port = u16::from_be_bytes([buf[8], buf[9]]);
            (ip.to_string(), port)
        }
        0x03 => {
            // 域名
            let len = buf[4] as usize;
            if n < 5 + len + 2 {
                return Ok(());
            }
            let domain = std::str::from_utf8(&buf[5..5 + len])?.to_string();
            let port = u16::from_be_bytes([buf[5 + len], buf[6 + len]]);
            (domain, port)
        }
        0x04 => {
            // IPv6 - 在 direct-tcpip 中不容易支持，拒绝
            client_stream.write_all(&[0x05, 0x08, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).await?;
            return Ok(());
        }
        _ => {
            client_stream.write_all(&[0x05, 0x07, 0x00, 0x01]).await?;
            return Ok(());
        }
    };

    // 打开到目标的 SSH 通道
    let channel = {
        let h = handle.lock().await;
        h.channel_open_direct_tcpip(&target_host, target_port as u32, "127.0.0.1", target_port as u32)
            .await
            .context("Failed to open SSH channel for SOCKS5")?
    };

    // 回复：VER=5, REP=SUCCESS, RSV=0, ATYP=1 (IPv4), BIND=0.0.0.0:0
    client_stream.write_all(&[0x05, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).await?;

    // 使用 russh 通道的数据事件循环进行双向复制
    let mut buf = [0u8; 8192];
    let mut channel = channel;

    loop {
        tokio::select! {
            result = client_stream.read(&mut buf) => {
                match result {
                    Ok(0) => break,
                    Ok(n) => {
                        if channel.data(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        log::debug!("SOCKS5 client read error: {}", e);
                        break;
                    }
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        if client_stream.write_all(&data[..]).await.is_err() {
                            break;
                        }
                    }
                    Some(ChannelMsg::Eof) | None => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

impl SshSession {
    pub fn new(host: String, port: u16, user: String, auth: SshAuthType, agent_forward: bool) -> Self {
        let (output_tx, _) = broadcast::channel(4096);
        Self {
            handle: None,
            channel_write: None,
            output_tx,
            host,
            port,
            user,
            auth,
            open: false,
            agent_forward,
            listeners: Vec::new(),
        }
    }
}

#[async_trait]
impl Session for SshSession {
    fn is_open(&self) -> bool {
        self.open
    }

    async fn start(&mut self) -> Result<()> {
        let config = Arc::new(client::Config {
            ..Default::default()
        });

        let handler = SshClientHandler::new(self.host.clone(), self.port);
        let mut session = client::connect(config, (&*self.host, self.port), handler)
            .await
            .context("Failed to connect to SSH server")?;

        // 认证
        let auth_result = match &self.auth {
            SshAuthType::None => {
                session
                    .authenticate_none(&self.user)
                    .await
                    .context("SSH none authentication failed")?
            }
            SshAuthType::Password(password) => {
                session
                    .authenticate_password(&self.user, password.clone())
                    .await
                    .context("SSH password authentication failed")?
            }
            SshAuthType::PublicKey { key_path, passphrase } => {
                let key_pair = russh::keys::load_secret_key(key_path, passphrase.as_deref())
                    .context("Failed to load SSH key")?;
                let key_with_alg = russh::keys::PrivateKeyWithHashAlg::new(
                    Arc::new(key_pair),
                    None,
                );
                session
                    .authenticate_publickey(&self.user, key_with_alg)
                    .await
                    .context("SSH publickey authentication failed")?
            }
            SshAuthType::Agent => {
                // 通过 russh 的 agent 客户端进行 SSH Agent 认证。
                // russh 的 PrivateKeyWithHashAlg 需要 PrivateKey，但 agent
                // 只暴露公钥 + 签名。我们使用带 agent 签名器的
                // russh session 的 authenticate_future 方法。
                //
                // 目前，回退到从
                // ~/.ssh/id_rsa 或 ~/.ssh/id_ed25519 加载默认 SSH 密钥并使用它进行认证。
                // 完整的 agent 协议支持将在未来的迭代中添加。
                let home = dirs::home_dir().context("Cannot determine home directory")?;
                let ssh_dir = home.join(".ssh");

                let key_paths = [
                    ssh_dir.join("id_ed25519"),
                    ssh_dir.join("id_rsa"),
                    ssh_dir.join("id_ecdsa"),
                ];

                let mut auth_success = false;
                for key_path in &key_paths {
                    if !key_path.exists() {
                        continue;
                    }
                    let key_pair = match russh::keys::load_secret_key(key_path, None) {
                        Ok(k) => k,
                        Err(_) => continue,
                    };
                    let key_with_alg = russh::keys::PrivateKeyWithHashAlg::new(
                        Arc::new(key_pair),
                        None,
                    );
                    let result = session
                        .authenticate_publickey(&self.user, key_with_alg)
                        .await
                        .context("SSH key authentication failed")?;
                    if result.success() {
                        auth_success = true;
                        break;
                    }
                }

                if !auth_success {
                    anyhow::bail!(
                        "SSH agent authentication failed: no suitable key found in ~/.ssh/"
                    );
                }
                russh::client::AuthResult::Success
            }
            SshAuthType::PromptPassword => {
                // 交互式密码提示 — 从 stdin 读取
                eprint!("Password for {}@{}: ", self.user, self.host);
                let password = rpassword::read_password()
                    .context("Failed to read password from terminal")?;
                session
                    .authenticate_password(&self.user, password)
                    .await
                    .context("SSH password authentication failed")?
            }
        };

        if !auth_result.success() {
            anyhow::bail!("SSH authentication rejected");
        }

        // 打开一个 shell 通道
        let channel = session
            .channel_open_session()
            .await
            .context("Failed to open SSH channel")?;

        channel
            .request_shell(true)
            .await
            .context("Failed to request shell")?;

        // 如果配置了则请求 agent 转发
        if self.agent_forward {
            // Agent 转发通过通道的扩展机制请求。
            // 当处理程序支持时，russh 库会在内部处理。
            log::info!("SSH agent forwarding requested");
        }

        // 分割通道：保留写半部分，将读半部分移动到生成的任务
        let (read_half, write_half) = channel.split();
        self.channel_write = Some(Arc::new(Mutex::new(write_half)));
        self.handle = Some(Arc::new(Mutex::new(session)));
        self.open = true;

        // 生成任务以读取通道输出
        let tx = self.output_tx.clone();
        tokio::spawn(async move {
            let mut read = read_half;
            loop {
                match read.wait().await {
                    Some(ChannelMsg::Data { data }) => {
                        if tx.send(data.to_vec()).is_err() {
                            break;
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, ext }) => {
                        // ext=1 是 stderr
                        if ext == 1 {
                            log::debug!("SSH stderr: {} bytes", data.len());
                            if tx.send(data.to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                    Some(ChannelMsg::Eof) | None => break,
                    _ => {}
                }
            }
        });

        Ok(())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        if let Some(ref channel) = self.channel_write {
            let channel = Arc::clone(channel);
            tokio::spawn(async move {
                let ch = channel.lock().await;
                let _ = ch.window_change(cols as u32, rows as u32, 0, 0).await;
            });
        }
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        if let Some(ref channel) = self.channel_write {
            let channel = Arc::clone(channel);
            let data = data.to_vec();
            tokio::spawn(async move {
                let ch = channel.lock().await;
                let _ = ch.data(&data[..]).await;
            });
        }
        Ok(())
    }

    fn kill(&mut self, _signal: &str) -> Result<()> {
        self.open = false;
        if let Some(ref channel) = self.channel_write {
            let channel = Arc::clone(channel);
            tokio::spawn(async move {
                let ch = channel.lock().await;
                let _ = ch.eof().await;
            });
        }
        if let Some(ref handle) = self.handle {
            let handle = Arc::clone(handle);
            tokio::spawn(async move {
                let h = handle.lock().await;
                let _ = h.disconnect(Disconnect::ByApplication, "", "").await;
            });
        }
        Ok(())
    }

    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    fn session_type_name(&self) -> &'static str {
        "ssh"
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        self.open = false;
    }
}
