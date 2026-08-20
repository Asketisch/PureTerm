//! `pureterm ssh tunnel <user@host>`——GAP-5.2 DoD:经网关开启一条
//! russh 支撑的 SOCKS5 隧道,由常驻 `puretermd` 持有。该隧道
//! 跨 GUI 重启仍存活,并把多个客户端连接复用在一条 SSH
//! 会话上。这是 `ssh_cli::connect` 的守护进程直连版。
//!
//! 从任何地方经它连接:
//!   `curl --socks5-hostname 127.0.0.1:<port> https://ifconfig.io`
//! `--socks5-hostname`(而非 `--socks5`)让客户端原样发送主机名,
//! 由*网关*解析 DNS——这正是仅有网关可见的 internal.corp 主机
//! 变为可达的原因。

use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::daemon_client;
use crate::ssh_cli::parse_target;

/// 启动一条隧道。确保守护进程就绪,调用 `tunnel.start`,然后轮询
/// `tunnel.list`,直到隧道离开 `connecting` 状态(或超时)。
pub fn start(
    target: &str,
    port: Option<u16>,
    identity: Option<String>,
    password: Option<String>,
    local_port: u16,
    remote_dns: bool,
    forward_agent: bool,
) {
    if let Err(msg) = run_start(target, port, identity, password, local_port, remote_dns, forward_agent) {
        eprintln!("[pureterm tunnel] {msg}");
        std::process::exit(1);
    }
}

fn run_start(
    target: &str,
    port: Option<u16>,
    identity: Option<String>,
    mut password: Option<String>,
    local_port: u16,
    remote_dns: bool,
    forward_agent: bool,
) -> Result<(), String> {
    let (user, host) = parse_target(target)
        .ok_or_else(|| format!("invalid target {target:?}; expected [user@]host"))?;

    // 若既未给密钥也未给密码,则在 stdin 上提示输入(明文回显——MVP 阶段;
    // 提供 --password 或 --identity 可避免该提示)。CLI 始终
    // 向守护进程递交凭据,因此守护进程侧的 need_auth 停驻仅
    // 能通过原始 JSON-RPC / 未来的 GUI 触达。带 `-A` 时守护进程会尝试
    // ssh-agent 密钥,故跳过提示。
    if identity.is_none() && password.is_none() && !forward_agent {
        print!("[pureterm tunnel] password for {target}: ");
        io::stdout().flush().ok();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_ok() {
            let pw = line.trim_end_matches(['\n', '\r']).to_string();
            if !pw.is_empty() {
                password = Some(pw);
            }
        }
    }

    let socket = daemon_client::ensure_daemon().map_err(|e| format!("daemon unavailable: {e}"))?;

    let tunnel_id = daemon_client::tunnel_start(
        &socket,
        &host,
        user.as_deref(),
        port,
        identity.as_deref(),
        password.as_deref(),
        local_port,
        remote_dns,
        forward_agent,
    )
    .map_err(|e| format!("tunnel start failed: {e}"))?;

    eprintln!(
        "[pureterm tunnel] starting {target} → SOCKS5 on 127.0.0.1:{local_port} (tunnel {tunnel_id})"
    );

    // 轮询终止状态(连接/认证在守护进程中异步进行)。
    let started = Instant::now();
    let deadline = Duration::from_secs(15);
    let mut last = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let tunnels = daemon_client::tunnel_list(&socket).map_err(|e| format!("tunnel list: {e}"))?;
        let mine = tunnels.iter().find(|t| t.get("id").and_then(|v| v.as_str()) == Some(&tunnel_id));
        let Some(info) = mine else {
            return Err(format!("tunnel {tunnel_id} vanished"));
        };
        let status = info
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("connecting")
            .to_string();
        last = status.clone();
        let local_addr = info
            .get("local_addr")
            .and_then(|v| v.as_str())
            .unwrap_or(&format!("127.0.0.1:{local_port}"))
            .to_string();
        match status.as_str() {
            "connecting" if started.elapsed() < deadline => continue,
            "connected" => {
                println!("[pureterm tunnel] connected: {target} (tunnel {tunnel_id})");
                println!("  SOCKS5 listener: {local_addr}");
                println!("  test: curl --socks5-hostname {local_addr} https://ifconfig.io");
                println!("  (tunnel persists in puretermd; stop with: pureterm ssh tunnel-stop {tunnel_id})");
                return Ok(());
            }
            "need_auth" => {
                return Err(format!(
                    "tunnel {tunnel_id} needs auth — supply --identity or --password (raw RPC: tunnel.provide_auth)"
                ));
            }
            "failed" => {
                let reason = info
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                return Err(format!("tunnel {tunnel_id} failed: {reason}"));
            }
            _ => break, // 未知状态 → 停止轮询
        }
    }
    Err(format!("tunnel {tunnel_id} did not connect (last status: {last})"))
}

/// 按 id 停止一条隧道。
pub fn stop(id: &str) {
    let socket = match daemon_client::ensure_daemon() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[pureterm tunnel] daemon unavailable: {e}");
            std::process::exit(1);
        }
    };
    match daemon_client::tunnel_stop(&socket, id) {
        Ok(true) => println!("[pureterm tunnel] stopped {id}"),
        Ok(false) => {
            eprintln!("[pureterm tunnel] no such tunnel {id}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("[pureterm tunnel] stop failed: {e}");
            std::process::exit(1);
        }
    }
}

/// 列出所有活跃隧道。
pub fn list() {
    let socket = match daemon_client::ensure_daemon() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[pureterm tunnel] daemon unavailable: {e}");
            std::process::exit(1);
        }
    };
    let tunnels = match daemon_client::tunnel_list(&socket) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[pureterm tunnel] list failed: {e}");
            std::process::exit(1);
        }
    };
    if tunnels.is_empty() {
        println!("no tunnels");
        return;
    }
    println!(
        "{:<22} {:<24} {:<6} {:<24} {:<12}  {}",
        "ID", "HOST", "PORT", "LOCAL", "STATUS", "ERROR"
    );
    for t in &tunnels {
        let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let host = t.get("host").and_then(|v| v.as_str()).unwrap_or("?");
        let user = t.get("user").and_then(|v| v.as_str()).unwrap_or("");
        let port = t.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
        let local = t.get("local_addr").and_then(|v| v.as_str()).unwrap_or("?");
        let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let err = t.get("error").and_then(|v| v.as_str()).unwrap_or("");
        let host_user = if user.is_empty() { host.to_string() } else { format!("{user}@{host}") };
        println!("{:<22} {:<24} {:<6} {:<24} {:<12}  {}", id, host_user, port, local, status, err);
    }
}
