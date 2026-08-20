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

/// russh 客户端处理器。覆写了 `check_server_key`(默认
/// 实现会拒绝所有主机密钥);为 GAP-5.4 覆写
/// `server_channel_open_agent_forward`,把服务器打开的 `auth-agent@openssh.com` 通道桥接到
/// 本地 ssh-agent socket。
struct ClientHandler {
    /// 已解析的 `$SSH_AUTH_SOCK` 路径;仅当该隧道请求了
    /// agent 转发时存在。回调克隆它以拉起桥接。
    agent_sock: Option<PathBuf>,
    /// GAP-5.2 TOFU 主机密钥存储。`None`(单元测试/未配置)保持
    /// 遗留的接受一切行为;否则 `check_server_key` 查询它。
    known_hosts: Option<Arc<KnownHosts>>,
    host: String,
    port: u16,
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    /// GAP-5.2 TOFU:首见时接受服务器密钥(并记录),或重复
    /// 出现且匹配时接受;失配则拒绝(严格——守护进程没有提示
    /// 路径,拒绝经由隧道的 `Failed` 状态浮出)。未配置
    /// 存储时回退为接受一切(测试/未配置)。
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

    /// GAP-5.4:服务器向我们打开了 `auth-agent@openssh.com` 通道
    ///(即远端想用我们的 agent)。用双向拷贝把它接到本地
    /// ssh-agent Unix socket,镜像 `serve_socks5`。
    /// 未解析到 agent socket(未请求转发)时丢弃该
    /// 通道——russh 默认是空操作,因此这是尽力而为。
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
        // 双向拷贝:SSH channel 流 ↔ agent Unix socket。
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

/// 纯主机密钥决策(可单测)。返回连接是否应
/// 继续:无存储(遗留/测试/未配置)、`Match` 或
/// `Unknown`(TOFU 记录 + 接受)时接受;`Mismatch` 时严格拒绝(守护进程
/// 没有交互提示路径,失配经由隧道的
/// `Failed` 状态浮出)。
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
    // 离开 NeedAuth 状态时清除驻留的认证 sender。
    if !matches!(status, TunnelLive::NeedAuth) {
        e.auth_tx = None;
    }
    e.status = status;
}

/// 驱动一条隧道:TCP+SSH 连接 → 认证 → SOCKS5 accept 循环。
/// 作为 runtime 任务运行;在每次状态转换时更新 `entry.status`。
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

    // SSH 连接 + 认证。
    let mut config = client::Config::default();
    // R6:keepalive 使死网关浮出水面(服务器停止应答 → russh
    // 在连续错过 keepalive_max 次后关闭),而非让隧道无声挂死。
    config.keepalive_interval = Some(std::time::Duration::from_secs(30));
    let config = Arc::new(config);

    let addr = format_connect_addr(&host, port);
    // GAP-5.4:预先解析本地 agent socket(配置覆盖 →
    // 继承的 $SSH_AUTH_SOCK → macOS launchctl),使处理器能桥接
    // 服务器打开的 agent 通道,也使 agent 密钥认证(下文)能到达它。
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

    // `Handle` 是 Send 但非 Sync(其应答接收端不是 Sync),因此包进
    // tokio Mutex 以跨每连接任务共享。守卫只在短暂的
    // `channel_open_direct_tcpip` 调用期间持有,拷贝期间绝不持有。
    let session: Arc<AsyncMutex<Handle<ClientHandler>>> = Arc::new(AsyncMutex::new(handle));

    // GAP-5.4 B3:在会话通道上请求 agent 转发,使服务器
    // 能向我们打开 `auth-agent@openssh.com` 通道(在
    // `ClientHandler::server_channel_open_agent_forward` 中桥接)。为隧道
    // 生命周期持有该通道,使请求持续有效。
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

/// 用手头的任一凭据认证。任务应停止(状态已设置)时返回 `None`,
/// 认证结果返回 `Some(bool)`。
async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    identity_file: &Option<String>,
    password: &Option<String>,
    entry: &Mutex<TunnelEntry>,
) -> Option<bool> {
    if let Some(idf) = identity_file.as_ref() {
        // 公钥认证。给出 `password` 时兼作密钥口令。
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
        // GAP-5.4 B2:先尝试本地 ssh-agent 的密钥(免密),失败再回退
        // 到密码 / NeedAuth。agent 不可用、无身份或
        // 无任何密钥获授权时向下贯穿。
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
            // 未提供凭据 → 驻留在 NeedAuth 直到 `provide_auth`。
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

/// GAP-5.4 B2:用本地 ssh-agent 持有的密钥认证
///(`authenticate_publickey_with`,agent 作为 `Signer`)。
/// 成功返回 `Some(true)`;agent 不可用、没有身份或
/// 无密钥被接受时返回 `None`——调用方因此向下贯穿到
/// 密码 / NeedAuth。
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

/// 服务一个 SOCKS5 客户端:协商免认证、解析 CONNECT、打开 SSH
/// direct-tcpip 通道,然后双向泵送字节直到任一侧关闭。
async fn serve_socks5(
    session: Arc<AsyncMutex<Handle<ClientHandler>>>,
    mut tcp: tokio::net::TcpStream,
    peer: std::net::SocketAddr,
    remote_dns: bool,
) -> std::io::Result<()> {
    // ── 方法协商:VER、NMETHODS、METHODS… ─────────────────────────────────────────────
    let mut hdr = [0u8; 2];
    tcp.read_exact(&mut hdr).await?;
    if hdr[0] != SOCKS5 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not SOCKS5"));
    }
    let mut methods = vec![0u8; hdr[1] as usize];
    tcp.read_exact(&mut methods).await?;
    if !methods.contains(&0x00) {
        // 客户端未提供“免认证”方法 → 0xFF = 无可接受方法。
        let _ = tcp.write_all(&[SOCKS5, 0xFF]).await;
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "client requires auth"));
    }
    tcp.write_all(&[SOCKS5, 0x00]).await?; // select no-auth

    // ── 请求:VER、CMD、RSV、ATYP、DST.ADDR、DST.PORT ──────────────────────────────────
    let mut req = [0u8; 4];
    tcp.read_exact(&mut req).await?;
    if req[0] != SOCKS5 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad request version"));
    }
    if req[1] != 0x01 {
        // 仅支持 CONNECT(无 BIND/UDP ASSOCIATE)→ 0x07。
        let _ = tcp.write_all(&[SOCKS5, 0x07, 0, 0x01, 0, 0, 0, 0, 0, 0]).await;
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "only CONNECT"));
    }
    let host = match read_dst_addr(&mut tcp, req[3]).await? {
        Some(h) => h,
        None => {
            // 不支持的 ATYP → 0x08。
            let _ = tcp.write_all(&[SOCKS5, 0x08, 0, 0x01, 0, 0, 0, 0, 0, 0]).await;
            return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "bad ATYP"));
        }
    };
    let mut port_buf = [0u8; 2];
    tcp.read_exact(&mut port_buf).await?;
    let port = u16::from_be_bytes(port_buf);

    // `--no-remote-dns`:在本地解析目标(使被查询的域名
    // 不泄漏给网关),把字面 IP 交给网关。默认
    //(remote_dns=true)时域名按原样转发做远端
    // 解析(socks5h / `--socks5-hostname`)。IP 字面量永不需要 DNS。
    let connect_host = if forward_verbatim(remote_dns, &host) {
        host.clone()
    } else {
        match resolve_host_to_ip(&host, port).await {
            Some(ip) => ip,
            None => {
                // 主机不可达 → 0x04。
                let _ = tcp.write_all(&[SOCKS5, 0x04, 0, 0x01, 0, 0, 0, 0, 0, 0]).await;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    format!("local DNS resolve failed for {host}"),
                ));
            }
        }
    };

    // ── 打开 SSH direct-tcpip 通道 ───────────────
    let channel = {
        let g = session.lock().await;
        g.channel_open_direct_tcpip(connect_host.clone(), port as u32, peer.ip().to_string(), peer.port() as u32)
            .await
    };
    let channel = match channel {
        Ok(c) => c,
        Err(e) => {
            // 通用 SOCKS 服务器失败 → 0x01。
            let _ = tcp.write_all(&[SOCKS5, 0x01, 0, 0x01, 0, 0, 0, 0, 0, 0]).await;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("channel_open_direct_tcpip({connect_host}:{port}): {e}"),
            ));
        }
    };

    // 成功应答:绑定的地址/端口为零(客户端对 CONNECT 会忽略)。
    tcp.write_all(&[SOCKS5, 0x00, 0, 0x01, 0, 0, 0, 0, 0, 0]).await?;

    // ── 双向拷贝:TCP ↔ SSH channel 流 ───────────────────────────────────────────────
    let stream = channel.into_stream();
    let (mut tcp_rd, mut tcp_wr) = tokio::io::split(tcp);
    let (mut ssh_rd, mut ssh_wr) = tokio::io::split(stream);
    let c2s = tokio::io::copy(&mut tcp_rd, &mut ssh_wr);
    let s2c = tokio::io::copy(&mut ssh_rd, &mut tcp_wr);
    let _ = tokio::join!(c2s, s2c); // finish when either direction ends
    Ok(())
}

/// 读取给定 ATYP 的 SOCKS5 DST.ADDR 字段。返回主机字符串
///(ATYP=3 时为字面域名,由 SSH 服务器在远端解析);不支持/
/// 畸形的 ATYP 返回 None。
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

/// 判断 `host` 应按原样转发给网关(由其远端解析——socks5h /
/// `--socks5-hostname` 的默认),还是在打开通道前
/// 本地解析(`--no-remote-dns`,使被查询的域名
/// 不泄漏给网关)。IP 字面量永不需要 DNS。
pub(super) fn forward_verbatim(remote_dns: bool, host: &str) -> bool {
    remote_dns || host.parse::<std::net::IpAddr>().is_ok()
}

/// 在本地把主机名解析为第一个 A/AAAA 地址(供 `--no-remote-dns`)。
/// 解析失败返回 `None`。调用方以 [`forward_verbatim`] 门控,
/// 只有真实域名会走到这里;IP 字面量无需查找即可解析。
async fn resolve_host_to_ip(host: &str, port: u16) -> Option<String> {
    tokio::net::lookup_host((host, port))
        .await
        .ok()?
        .next()
        .map(|sa| sa.ip().to_string())
}

/// 为 russh 的 `client::connect` 格式化 `host:port` 连接地址。IPv6
/// 字面量(含 ':')必须加方括号——`[2001:db8::1]:22`——否则
/// 字符串有歧义、无法解析为 socket 地址(连接失败)。
pub(super) fn format_connect_addr(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
