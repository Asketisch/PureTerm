//! `pureterm ssh tunnel <user@host>` — GAP-5.2 DoD: open a russh-backed SOCKS5
//! tunnel through a gateway, owned by the persistent `puretermd`. The tunnel
//! survives GUI restart and multiplexes many client connections over one SSH
//! session. This is the daemon-direct counterpart of `ssh_cli::connect`.
//!
//! Connect through it from anywhere:
//!   `curl --socks5-hostname 127.0.0.1:<port> https://ifconfig.io`
//! `--socks5-hostname` (not `--socks5`) makes the client send the hostname
//! verbatim, so the *gateway* resolves DNS — that's how internal.corp hosts only
//! the gateway can see become reachable.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::daemon_client;
use crate::ssh_cli::parse_target;

/// Start a tunnel. Ensures the daemon, calls `tunnel.start`, then polls
/// `tunnel.list` until the tunnel leaves the `connecting` state (or times out).
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

    // If neither a key nor a password was given, prompt on stdin (echoed — MVP;
    // supply --password or --identity to avoid the prompt). The CLI always
    // hands the daemon a credential, so the daemon-side need_auth park is only
    // reachable via raw JSON-RPC / a future GUI. With `-A` the daemon will try
    // ssh-agent keys, so skip the prompt.
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

    // Poll for a terminal status (the connect/auth runs async in the daemon).
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
            _ => break, // unknown status → stop polling
        }
    }
    Err(format!("tunnel {tunnel_id} did not connect (last status: {last})"))
}

/// Stop a tunnel by id.
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

/// List active tunnels.
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
