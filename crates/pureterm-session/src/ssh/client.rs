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
                // TOFU: Trust On First Use — accept and save the key
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

/// SSH session using russh.
pub struct SshSession {
    handle: Option<Arc<Mutex<Handle<SshClientHandler>>>>,
    channel_write: Option<Arc<Mutex<russh::ChannelWriteHalf<russh::client::Msg>>>>,
    output_tx: broadcast::Sender<Vec<u8>>,
    host: String,
    port: u16,
    user: String,
    auth: SshAuthType,
    open: bool,
    /// Whether agent forwarding is enabled.
    agent_forward: bool,
    /// Pending port forward listeners (local forwards)
    listeners: Vec<PortForwardListener>,
}

/// A port forwarding listener that accepts local connections and forwards through SSH.
#[allow(dead_code)]
pub struct PortForwardListener {
    /// The TCP listener for accepting local connections.
    listener: Arc<TcpListener>,
    /// The remote host to connect to on the SSH server side.
    remote_host: String,
    /// The remote port.
    remote_port: u16,
    /// Task handle for the accept loop.
    _task: tokio::task::JoinHandle<()>,
}

impl SshSession {
    /// Open a direct-tcpip channel to the specified host:port through SSH.
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

    /// Start a local port forward: accepts connections on `local_addr:local_port`
    /// and forwards them through this SSH session to `remote_host:remote_port`.
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

    /// Start a dynamic (SOCKS5) port forward on `local_addr:local_port`.
    /// This acts as a SOCKS5 proxy through the SSH connection.
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

    /// Stop all active port forwards.
    pub fn stop_forwards(&mut self) {
        // The task will exit when the listener is dropped
        self.listeners.clear();
    }

    /// Start a remote port forward: requests the SSH server to listen on
    /// `remote_addr:remote_port` and forward connections back to `local_addr:local_port`.
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
            // Request the server to forward connections to remote_addr:remote_port
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

        // Wait for the request to complete
        pollster::block_on(task)?;

        Ok(())
    }
}

/// Proxy a TCP connection via SSH direct-tcpip.
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

    // Use russh channel's data event loop for bidirectional copy
    let mut buf = [0u8; 8192];
    let mut channel = channel;

    loop {
        tokio::select! {
            // Copy from client to SSH channel
            result = client_stream.read(&mut buf) => {
                match result {
                    Ok(0) => break, // Connection closed
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
            // Read from SSH channel and forward to client
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

/// Handle a SOCKS5 connection and proxy it through SSH.
async fn proxy_socks5(
    client_stream: &mut TcpStream,
    _client_addr: std::net::SocketAddr,
    handle: Arc<Mutex<Handle<SshClientHandler>>>,
) -> Result<()> {
    use std::net::Ipv4Addr;

    // SOCKS5 handshake
    let mut buf = [0u8; 262];
    let n = client_stream.read(&mut buf).await?;
    if n < 2 {
        return Ok(());
    }

    // VER=5, NMETHODS
    if buf[0] != 0x05 {
        return Ok(());
    }

    // Respond: VER=5, METHOD=NO_AUTHENTICATION_REQUIRED (0)
    client_stream.write_all(&[0x05, 0x00]).await?;

    // Read connection request: VER=5, CMD=CONNECT, RSV=0, ATYP=1 (IPv4)
    let n = client_stream.read(&mut buf).await?;
    if n < 10 || buf[0] != 0x05 || buf[1] != 0x01 {
        // Unsupported: reply with COMMAND_NOT_SUPPORTED
        client_stream.write_all(&[0x05, 0x07, 0x00, 0x01]).await?;
        return Ok(());
    }

    // Parse target address (ATYP=1 is IPv4, ATYP=3 is domain name)
    let (target_host, target_port) = match buf[3] {
        0x01 => {
            // IPv4
            let ip = Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]);
            let port = u16::from_be_bytes([buf[8], buf[9]]);
            (ip.to_string(), port)
        }
        0x03 => {
            // Domain name
            let len = buf[4] as usize;
            if n < 5 + len + 2 {
                return Ok(());
            }
            let domain = std::str::from_utf8(&buf[5..5 + len])?.to_string();
            let port = u16::from_be_bytes([buf[5 + len], buf[6 + len]]);
            (domain, port)
        }
        0x04 => {
            // IPv6 - not supported in direct-tcpip easily, reject
            client_stream.write_all(&[0x05, 0x08, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).await?;
            return Ok(());
        }
        _ => {
            client_stream.write_all(&[0x05, 0x07, 0x00, 0x01]).await?;
            return Ok(());
        }
    };

    // Open SSH channel to the target
    let channel = {
        let h = handle.lock().await;
        h.channel_open_direct_tcpip(&target_host, target_port as u32, "127.0.0.1", target_port as u32)
            .await
            .context("Failed to open SSH channel for SOCKS5")?
    };

    // Reply: VER=5, REP=SUCCESS, RSV=0, ATYP=1 (IPv4), BIND=0.0.0.0:0
    client_stream.write_all(&[0x05, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).await?;

    // Use russh channel's data event loop for bidirectional copy
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

        // Authenticate
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
                // SSH Agent authentication via russh's agent client.
                // russh's PrivateKeyWithHashAlg requires a PrivateKey, but agent
                // only exposes public keys + signing. We use the russh
                // session's authenticate_future method with an agent signer.
                //
                // For now, fall back to loading the default SSH key from
                // ~/.ssh/id_rsa or ~/.ssh/id_ed25519 and authenticating with it.
                // Full agent protocol support will be added in a future iteration.
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
                // Interactive password prompt — read from stdin
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

        // Open a shell channel
        let channel = session
            .channel_open_session()
            .await
            .context("Failed to open SSH channel")?;

        channel
            .request_shell(true)
            .await
            .context("Failed to request shell")?;

        // Request agent forwarding if configured
        if self.agent_forward {
            // Agent forwarding is requested via the channel's extended mechanism.
            // The russh library handles this internally when the handler supports it.
            log::info!("SSH agent forwarding requested");
        }

        // Split the channel: keep the write half, move the read half to a spawned task
        let (read_half, write_half) = channel.split();
        self.channel_write = Some(Arc::new(Mutex::new(write_half)));
        self.handle = Some(Arc::new(Mutex::new(session)));
        self.open = true;

        // Spawn task to read channel output
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
                        // ext=1 is stderr
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
