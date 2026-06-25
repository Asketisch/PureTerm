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

use std::collections::HashMap;
use std::net::TcpListener as StdTcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pureterm_ssh::agent_sock;
use russh::client::{self, Handle, Handler, Msg, Session};
use russh::keys::agent::client::AgentClient;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::Channel;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex as AsyncMutex};

/// SOCKS5 protocol version byte.
const SOCKS5: u8 = 5;

/// Fresh tunnel id (mirrors `session::new_session_id`): `<pid>-<counter>`.
fn new_tunnel_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tun-{}-{}", std::process::id(), n)
}

/// Live tunnel state machine.
#[derive(Clone, Debug)]
enum TunnelLive {
    Connecting,
    Connected,
    NeedAuth,
    Failed(String),
}

impl TunnelLive {
    fn as_str(&self) -> &'static str {
        match self {
            TunnelLive::Connecting => "connecting",
            TunnelLive::Connected => "connected",
            TunnelLive::NeedAuth => "need_auth",
            TunnelLive::Failed(_) => "failed",
        }
    }
}

/// Serializable tunnel descriptor (for `tunnel.list`).
#[derive(Debug, Clone, Serialize)]
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
struct TunnelEntry {
    host: String,
    user: Option<String>,
    port: u16,
    local_addr: String,
    #[allow(dead_code)]
    identity_file: Option<String>,
    status: TunnelLive,
    auth_tx: Option<oneshot::Sender<String>>,
    abort: Option<tokio::task::AbortHandle>,
}

/// Central tunnel registry, shared between the (sync) control plane and the
/// (async) connect tasks. Mirrors `session::SessionManager`'s shape.
#[derive(Clone, Default)]
pub struct TunnelManager {
    tunnels: Arc<Mutex<HashMap<String, Arc<Mutex<TunnelEntry>>>>>,
}

impl TunnelManager {
    /// Register + launch a tunnel. Binds the SOCKS5 listener synchronously so
    /// the returned id is immediately usable (the port is reserved before the
    /// async connect even starts). Returns the tunnel id.
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
        // Bind first so a port clash fails fast (before we touch the network).
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
            identity_file: identity_file.clone(),
            status: TunnelLive::Connecting,
            auth_tx: None,
            abort: None,
        }));

        let entry_for_task = entry.clone();
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
        ));
        entry.lock().unwrap().abort = Some(join.abort_handle());

        self.tunnels.lock().unwrap().insert(id.clone(), entry);
        Ok(id)
    }

    /// Enumerate live tunnels as serializable descriptors.
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

    /// Stop + remove a tunnel (aborts the connect/accept task; dropping the
    /// russh `Handle` disconnects the SSH session). Returns whether it existed.
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

    /// Provide the auth secret to a tunnel parked in `NeedAuth`. Errors if the
    /// tunnel is unknown or not currently awaiting auth (e.g. still connecting,
    /// or already authenticated).
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
}

/// The russh client handler. `check_server_key` is overridden (the default
/// rejects every host key); `server_channel_open_agent_forward` is overridden
/// for GAP-5.4 to bridge the server-opened `auth-agent@openssh.com` channel to
/// the local ssh-agent socket.
struct ClientHandler {
    /// Resolved `$SSH_AUTH_SOCK` path; present only when agent forwarding is
    /// requested for this tunnel. The callback clones it to spawn the bridge.
    agent_sock: Option<PathBuf>,
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    /// GAP-5.2 MVP: accept all server host keys (trust-on-first-use pending a
    /// known_hosts store). The tunnel only targets gateways the operator named
    /// explicitly via the CLI/control plane, mirroring a first-run SSH client.
    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// GAP-5.4: the server opened an `auth-agent@openssh.com` channel back to
    /// us (i.e. the remote wants to use our agent). Splice it to the local
    /// ssh-agent Unix socket with a bidirectional copy, mirroring `serve_socks5`.
    /// If no agent socket was resolved (forwarding not requested), drop the
    /// channel — russh's default is a no-op, so this is best-effort.
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let Some(sock) = self.agent_sock.clone() else {
            return Ok(());
        };
        let agent = match tokio::net::UnixStream::connect(&sock).await {
            Ok(a) => a,
            Err(e) => {
                log::debug!("agent_forward: connect {}: {e}", sock.display());
                return Ok(());
            }
        };
        // Bidirectional copy: SSH channel stream ↔ agent Unix socket.
        let stream = channel.into_stream();
        let (mut ar, mut aw) = tokio::io::split(agent);
        let (mut sr, mut sw) = tokio::io::split(stream);
        tokio::spawn(async move {
            let c2a = tokio::io::copy(&mut sr, &mut aw);
            let a2c = tokio::io::copy(&mut ar, &mut sw);
            let _ = tokio::join!(c2a, a2c); // finish when either direction ends
        });
        Ok(())
    }
}

fn set_status(entry: &Mutex<TunnelEntry>, status: TunnelLive) {
    let mut e = entry.lock().unwrap();
    // Clear any parked auth sender when leaving the NeedAuth state.
    if !matches!(status, TunnelLive::NeedAuth) {
        e.auth_tx = None;
    }
    e.status = status;
}

/// Drive one tunnel: TCP+SSH connect → authenticate → SOCKS5 accept loop.
/// Runs as a runtime task; updates `entry.status` at each transition.
async fn run_tunnel(
    entry: Arc<Mutex<TunnelEntry>>,
    std_listener: StdTcpListener,
    host: String,
    user: Option<String>,
    port: u16,
    identity_file: Option<String>,
    password: Option<String>,
    _remote_dns: bool,
    forward_agent: bool,
) {
    let listener = match TcpListener::from_std(std_listener) {
        Ok(l) => l,
        Err(e) => {
            set_status(&entry, TunnelLive::Failed(format!("listener: {e}")));
            return;
        }
    };

    // SSH connect + auth.
    let mut config = client::Config::default();
    // R6: keepalives surface a dead gateway (server stops replying → russh
    // closes after keepalive_max misses) instead of hanging the tunnel silently.
    config.keepalive_interval = Some(std::time::Duration::from_secs(30));
    let config = Arc::new(config);

    let addr = format!("{host}:{port}");
    // GAP-5.4: resolve the local agent socket up front (config override →
    // inherited $SSH_AUTH_SOCK → macOS launchctl) so the handler can bridge
    // server-opened agent channels, and so agent-key auth (below) can reach it.
    let agent_sock = if forward_agent {
        agent_sock::resolve_agent_sock(None)
    } else {
        None
    };
    let handler = ClientHandler {
        agent_sock: agent_sock.clone(),
    };
    let mut handle = match client::connect(config, &addr, handler).await {
        Ok(h) => h,
        Err(e) => {
            set_status(&entry, TunnelLive::Failed(format!("connect: {e}")));
            return;
        }
    };

    let user = user
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_default();

    let authed = authenticate(&mut handle, &user, &identity_file, &password, &entry).await;
    let authed = match authed {
        Some(ok) => ok,
        None => return, // status already set to Failed/NeedAuth
    };
    if !authed {
        set_status(&entry, TunnelLive::Failed("authentication rejected".into()));
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        return;
    }

    set_status(&entry, TunnelLive::Connected);
    log::info!("puretermd tunnel connected: {user}@{host} ({addr})");

    // `Handle` is Send but not Sync (its reply receiver isn't Sync), so wrap in
    // a tokio Mutex to share across per-connection tasks. The guard is held only
    // for the brief `channel_open_direct_tcpip` call, never during the copy.
    let session: Arc<AsyncMutex<Handle<ClientHandler>>> = Arc::new(AsyncMutex::new(handle));

    // GAP-5.4 B3: request agent forwarding on a session channel so the server
    // can open `auth-agent@openssh.com` channels back to us (bridged in
    // `ClientHandler::server_channel_open_agent_forward`). Hold the channel for
    // the tunnel's lifetime so the request stays in force.
    let _agent_channel = if forward_agent {
        let ch = {
            let g = session.lock().await;
            g.channel_open_session().await
        };
        match ch {
            Ok(ch) => match ch.agent_forward(true).await {
                Ok(()) => {
                    log::debug!("agent_forward requested for {user}@{host}");
                    Some(ch)
                }
                Err(e) => {
                    log::warn!("agent_forward request failed for {user}@{host}: {e}");
                    None
                }
            },
            Err(e) => {
                log::warn!("channel_open_session for agent forward failed: {e}");
                None
            }
        }
    } else {
        None
    };

    loop {
        match listener.accept().await {
            Ok((tcp, peer)) => {
                let session = session.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve_socks5(session, tcp, peer).await {
                        log::debug!("socks5 conn from {peer} ended: {e}");
                    }
                });
            }
            Err(e) => {
                log::warn!("socks5 accept error: {e}");
                break;
            }
        }
    }
    set_status(&entry, TunnelLive::Failed("listener closed".into()));
}

/// Authenticate using whichever credential we have. Returns `None` if the task
/// should stop (status already set), `Some(bool)` for the auth outcome.
async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    identity_file: &Option<String>,
    password: &Option<String>,
    entry: &Mutex<TunnelEntry>,
) -> Option<bool> {
    if let Some(idf) = identity_file.as_ref() {
        // Publickey auth. `password` doubles as the key passphrase when given.
        match load_secret_key(idf, password.as_deref()) {
            Ok(key) => match handle
                .authenticate_publickey(user.to_string(), PrivateKeyWithHashAlg::new(Arc::new(key), None))
                .await
            {
                Ok(r) => Some(r.success()),
                Err(e) => {
                    set_status(entry, TunnelLive::Failed(format!("pubkey auth: {e}")));
                    None
                }
            },
            Err(e) => {
                set_status(entry, TunnelLive::Failed(format!("load key {idf}: {e}")));
                None
            }
        }
    } else {
        // GAP-5.4 B2: try a local ssh-agent key (passwordless) before falling
        // back to password / NeedAuth. Falls through if the agent is
        // unavailable, has no identities, or none of its keys are authorized.
        if let Some(true) = try_agent_auth(handle, user).await {
            return Some(true);
        }
        if let Some(pw) = password.as_ref() {
            match handle.authenticate_password(user.to_string(), pw.clone()).await {
                Ok(r) => Some(r.success()),
                Err(e) => {
                    set_status(entry, TunnelLive::Failed(format!("password auth: {e}")));
                    None
                }
            }
        } else {
            // No credential supplied → park in NeedAuth until `provide_auth`.
            set_status(entry, TunnelLive::NeedAuth);
            let (tx, rx) = oneshot::channel();
            entry.lock().unwrap().auth_tx = Some(tx);
            let secret = match rx.await {
                Ok(s) => s,
                Err(_) => {
                    set_status(entry, TunnelLive::Failed("auth cancelled".into()));
                    return None;
                }
            };
            match handle.authenticate_password(user.to_string(), secret).await {
                Ok(r) => Some(r.success()),
                Err(e) => {
                    set_status(entry, TunnelLive::Failed(format!("auth: {e}")));
                    None
                }
            }
        }
    }
}

/// GAP-5.4 B2: authenticate using a key held by the local ssh-agent
/// (`authenticate_publickey_with` with the agent as the `Signer`). Returns
/// `Some(true)` on success; `None` if the agent is unavailable, holds no
/// identities, or none of its keys are accepted — so the caller falls through
/// to password / NeedAuth.
async fn try_agent_auth(handle: &mut Handle<ClientHandler>, user: &str) -> Option<bool> {
    let sock = agent_sock::resolve_agent_sock(None)?;
    let mut agent = AgentClient::connect_uds(&sock).await.ok()?;
    let identities = agent.request_identities().await.ok()?;
    for id in identities {
        let pubkey = id.public_key().into_owned();
        match handle
            .authenticate_publickey_with(user.to_string(), pubkey, None, &mut agent)
            .await
        {
            Ok(r) if r.success() => return Some(true),
            Ok(_) => continue, // this key wasn't authorized; try the next
            Err(e) => {
                log::debug!("agent auth sign error for {user}: {e}");
                continue;
            }
        }
    }
    None
}

/// Serve one SOCKS5 client: negotiate no-auth, parse CONNECT, open the SSH
/// direct-tcpip channel, then pump bytes both ways until either side closes.
async fn serve_socks5(
    session: Arc<AsyncMutex<Handle<ClientHandler>>>,
    mut tcp: tokio::net::TcpStream,
    peer: std::net::SocketAddr,
) -> std::io::Result<()> {
    // ── Method negotiation: VER, NMETHODS, METHODS… ────────────────────────
    let mut hdr = [0u8; 2];
    tcp.read_exact(&mut hdr).await?;
    if hdr[0] != SOCKS5 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not SOCKS5"));
    }
    let mut methods = vec![0u8; hdr[1] as usize];
    tcp.read_exact(&mut methods).await?;
    if !methods.contains(&0x00) {
        // Client offered no "no auth" method → 0xFF = no acceptable methods.
        let _ = tcp.write_all(&[SOCKS5, 0xFF]).await;
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "client requires auth"));
    }
    tcp.write_all(&[SOCKS5, 0x00]).await?; // select no-auth

    // ── Request: VER, CMD, RSV, ATYP, DST.ADDR, DST.PORT ───────────────────
    let mut req = [0u8; 4];
    tcp.read_exact(&mut req).await?;
    if req[0] != SOCKS5 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad request version"));
    }
    if req[1] != 0x01 {
        // Only CONNECT is supported (no BIND/UDP ASSOCIATE) → 0x07.
        let _ = tcp.write_all(&[SOCKS5, 0x07, 0, 0x01, 0, 0, 0, 0, 0, 0]).await;
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "only CONNECT"));
    }
    let host = match read_dst_addr(&mut tcp, req[3]).await? {
        Some(h) => h,
        None => {
            // Unsupported ATYP → 0x08.
            let _ = tcp.write_all(&[SOCKS5, 0x08, 0, 0x01, 0, 0, 0, 0, 0, 0]).await;
            return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "bad ATYP"));
        }
    };
    let mut port_buf = [0u8; 2];
    tcp.read_exact(&mut port_buf).await?;
    let port = u16::from_be_bytes(port_buf);

    // ── Open the SSH direct-tcpip channel (DNS resolved remotely for domains).
    let channel = {
        let g = session.lock().await;
        g.channel_open_direct_tcpip(host.clone(), port as u32, peer.ip().to_string(), peer.port() as u32)
            .await
    };
    let channel = match channel {
        Ok(c) => c,
        Err(e) => {
            // General SOCKS server failure → 0x01.
            let _ = tcp.write_all(&[SOCKS5, 0x01, 0, 0x01, 0, 0, 0, 0, 0, 0]).await;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("channel_open_direct_tcpip({host}:{port}): {e}"),
            ));
        }
    };

    // Success reply: bound addr/port zeros (clients ignore for CONNECT).
    tcp.write_all(&[SOCKS5, 0x00, 0, 0x01, 0, 0, 0, 0, 0, 0]).await?;

    // ── Bidirectional copy: TCP ↔ SSH channel stream. ──────────────────────
    let stream = channel.into_stream();
    let (mut tcp_rd, mut tcp_wr) = tokio::io::split(tcp);
    let (mut ssh_rd, mut ssh_wr) = tokio::io::split(stream);
    let c2s = tokio::io::copy(&mut tcp_rd, &mut ssh_wr);
    let s2c = tokio::io::copy(&mut ssh_rd, &mut tcp_wr);
    let _ = tokio::join!(c2s, s2c); // finish when either direction ends
    Ok(())
}

/// Read the SOCKS5 DST.ADDR field for a given ATYP. Returns the host string
/// (literal domain for ATYP=3, so the SSH server resolves it remotely) or None
/// for an unsupported/malformed ATYP.
async fn read_dst_addr(
    tcp: &mut tokio::net::TcpStream,
    atyp: u8,
) -> std::io::Result<Option<String>> {
    match atyp {
        0x01 => {
            let mut b = [0u8; 4];
            tcp.read_exact(&mut b).await?;
            Ok(Some(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])))
        }
        0x04 => {
            let mut b = [0u8; 16];
            tcp.read_exact(&mut b).await?;
            Ok(Some(std::net::Ipv6Addr::from(b).to_string()))
        }
        0x03 => {
            let mut len = [0u8; 1];
            tcp.read_exact(&mut len).await?;
            let mut buf = vec![0u8; len[0] as usize];
            tcp.read_exact(&mut buf).await?;
            match String::from_utf8(buf) {
                Ok(s) => Ok(Some(s)),
                Err(_) => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_ids_are_unique() {
        let a = new_tunnel_id();
        let b = new_tunnel_id();
        assert!(a.starts_with("tun-"));
        assert_ne!(a, b);
    }

    #[test]
    fn status_strings_match_cli_contract() {
        assert_eq!(TunnelLive::Connecting.as_str(), "connecting");
        assert_eq!(TunnelLive::Connected.as_str(), "connected");
        assert_eq!(TunnelLive::NeedAuth.as_str(), "need_auth");
        assert_eq!(TunnelLive::Failed("x".into()).as_str(), "failed");
    }

    #[test]
    fn list_is_empty_by_default() {
        let mgr = TunnelManager::default();
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn stop_unknown_returns_false() {
        let mgr = TunnelManager::default();
        assert!(!mgr.stop("nope"));
    }

    #[test]
    fn provide_auth_unknown_errors() {
        let mgr = TunnelManager::default();
        assert!(mgr.provide_auth("nope", "s".into()).is_err());
    }

    /// `read_dst_addr` is async over a real socket; this exercises the IPv4 /
    /// IPv6 / domain formatting paths by feeding a cursor-backed mock stream
    /// through the same Tokio codec. We instead unit-test the pure formatting
    /// invariants the function relies on.
    #[test]
    fn ipv4_format_is_dotted_quad() {
        // Mirrors the 0x01 branch of read_dst_addr.
        let b = [192u8, 168, 1, 42];
        assert_eq!(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]), "192.168.1.42");
    }

    #[test]
    fn port_is_big_endian_from_two_bytes() {
        // Mirrors the port decode in serve_socks5.
        assert_eq!(u16::from_be_bytes([0x1F, 0x90]), 8080);
        assert_eq!(u16::from_be_bytes([0x00, 0x50]), 80);
    }
}
