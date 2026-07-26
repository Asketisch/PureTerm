//! SSH 客户端 + SOCKS5 转发逻辑:russh `Handler` 实现 (`ClientHandler`)、
//! TOFU host-key 决策 (`decide_host_key`)、连接/认证/接受循环 (`run_tunnel`)、
//! 认证助手 (`authenticate` / `try_agent_auth`)、SOCKS5 服务端 (`serve_socks5`)
//! 及其编解码助手。所有项 `pub(super)`,仅供 `manager::start` 调用入口
//! `run_tunnel`,其余为模块内部协作。

use std::net::TcpListener as StdTcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pureterm_ssh::agent_sock;
use russh::client::{self, Handle, Handler, Msg, Session};
use russh::keys::agent::client::AgentClient;
use russh::keys::ssh_key::HashAlg;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::Channel;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex as AsyncMutex};

use crate::known_hosts::{format_host_pattern, KeyCheck, KnownHosts};

use super::{TunnelEntry, TunnelLive, SOCKS5};

/// The russh client handler. `check_server_key` is overridden (the default
/// rejects every host key); `server_channel_open_agent_forward` is overridden
/// for GAP-5.4 to bridge the server-opened `auth-agent@openssh.com` channel to
/// the local ssh-agent socket.
struct ClientHandler {
    /// Resolved `$SSH_AUTH_SOCK` path; present only when agent forwarding is
    /// requested for this tunnel. The callback clones it to spawn the bridge.
    agent_sock: Option<PathBuf>,
    /// GAP-5.2 TOFU host-key store. `None` (unit tests / unconfigured) keeps the
    /// legacy accept-all behavior; otherwise `check_server_key` consults it.
    known_hosts: Option<Arc<KnownHosts>>,
    host: String,
    port: u16,
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    /// GAP-5.2 TOFU: accept the server key on first sight (recording it) or on a
    /// matching repeat; reject on a mismatch (strict — the daemon has no prompt
    /// path, so the rejection surfaces via the tunnel's `Failed` status). With
    /// no store configured, falls back to accept-all (tests/unconfigured).
    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let host_pattern = format_host_pattern(&self.host, self.port);
        Ok(decide_host_key(
            self.known_hosts.as_deref(),
            &host_pattern,
            server_public_key,
        ))
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

/// Pure host-key decision (unit-testable). Returns whether the connection should
/// proceed: accept on no-store (legacy/tests/unconfigured), `Match`, or
/// `Unknown` (TOFU record + accept); reject strictly on `Mismatch` (the daemon
/// has no interactive prompt path, so a mismatch surfaces via the tunnel's
/// `Failed` status).
pub(super) fn decide_host_key(
    kh: Option<&KnownHosts>,
    host_pattern: &str,
    key: &russh::keys::ssh_key::PublicKey,
) -> bool {
    let Some(kh) = kh else {
        return true; // no store configured (tests/unconfigured) → legacy accept-all
    };
    match kh.check_and_record(host_pattern, key) {
        KeyCheck::Match | KeyCheck::Unknown => true,
        KeyCheck::Mismatch(stored_fp) => {
            let live_fp = key.fingerprint(HashAlg::Sha256).to_string();
            log::warn!("host key mismatch for {host_pattern}: known={stored_fp} live={live_fp}");
            false
        }
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
pub(super) async fn run_tunnel(
    entry: Arc<Mutex<TunnelEntry>>,
    std_listener: StdTcpListener,
    host: String,
    user: Option<String>,
    port: u16,
    identity_file: Option<String>,
    password: Option<String>,
    remote_dns: bool,
    forward_agent: bool,
    known_hosts: Option<Arc<KnownHosts>>,
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

    let addr = format_connect_addr(&host, port);
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
        known_hosts,
        host: host.clone(),
        port,
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
                    if let Err(e) = serve_socks5(session, tcp, peer, remote_dns).await {
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
    remote_dns: bool,
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

    // `--no-remote-dns`: resolve the destination locally (so the queried domain
    // does not leak to the gateway) and hand the gateway a literal IP. With the
    // default (remote_dns=true) the domain is forwarded verbatim for remote
    // resolution (socks5h / `--socks5-hostname`). IP literals never need DNS.
    let connect_host = if forward_verbatim(remote_dns, &host) {
        host.clone()
    } else {
        match resolve_host_to_ip(&host, port).await {
            Some(ip) => ip,
            None => {
                // Host unreachable → 0x04.
                let _ = tcp.write_all(&[SOCKS5, 0x04, 0, 0x01, 0, 0, 0, 0, 0, 0]).await;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    format!("local DNS resolve failed for {host}"),
                ));
            }
        }
    };

    // ── Open the SSH direct-tcpip channel.
    let channel = {
        let g = session.lock().await;
        g.channel_open_direct_tcpip(connect_host.clone(), port as u32, peer.ip().to_string(), peer.port() as u32)
            .await
    };
    let channel = match channel {
        Ok(c) => c,
        Err(e) => {
            // General SOCKS server failure → 0x01.
            let _ = tcp.write_all(&[SOCKS5, 0x01, 0, 0x01, 0, 0, 0, 0, 0, 0]).await;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("channel_open_direct_tcpip({connect_host}:{port}): {e}"),
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

/// Whether `host` should be forwarded verbatim to the gateway (which then
/// resolves it remotely — the socks5h / `--socks5-hostname` default) or
/// resolved locally before the channel is opened (`--no-remote-dns`, so the
/// queried domain does not leak to the gateway). IP literals never need DNS.
pub(super) fn forward_verbatim(remote_dns: bool, host: &str) -> bool {
    remote_dns || host.parse::<std::net::IpAddr>().is_ok()
}

/// Resolve a hostname to its first A/AAAA address locally (for `--no-remote-dns`).
/// Returns `None` on resolution failure. Callers gate on [`forward_verbatim`] and
/// only reach this for real domains; IP literals would parse without a lookup.
async fn resolve_host_to_ip(host: &str, port: u16) -> Option<String> {
    tokio::net::lookup_host((host, port))
        .await
        .ok()?
        .next()
        .map(|sa| sa.ip().to_string())
}

/// Format a `host:port` connect address for russh's `client::connect`. An IPv6
/// literal (which contains ':') must be bracketed — `[2001:db8::1]:22` — or the
/// string is ambiguous and won't parse as a socket address (connect fails).
pub(super) fn format_connect_addr(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
