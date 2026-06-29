//! GAP-33: Port Scanner — periodic scan of listening TCP ports + process names.
//!
//! Unix uses `lsof` (reliable PID/name mapping). Results feed the right-sidebar
//! "Ports" tab; click copies `localhost:PORT` or opens a browser for HTTP ports.

use std::time::Duration;

use iced::futures::SinkExt;
use iced::Subscription;

use crate::app_config::PortScannerConfig;
use crate::iced_app::Message;

/// One listening socket row in the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListeningPort {
    pub port: u16,
    pub pid: u32,
    pub process: String,
    pub protocol: String,
}

/// Scan listening TCP ports. Returns entries sorted by port ascending.
pub fn scan_listening_ports(cfg: &PortScannerConfig) -> Vec<ListeningPort> {
    if !cfg.enabled {
        return Vec::new();
    }
    #[cfg(unix)]
    {
        let mut rows = parse_lsof_output(run_lsof());
        rows.retain(|r| r.port >= cfg.min_port);
        rows.sort_by_key(|r| r.port);
        rows.dedup_by_key(|r| r.port);
        rows
    }
    #[cfg(not(unix))]
    {
        let _ = cfg;
        Vec::new()
    }
}

#[cfg(unix)]
fn run_lsof() -> String {
    use std::process::Command;
    Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
}

#[cfg(unix)]
fn parse_lsof_output(output: String) -> Vec<ListeningPort> {
    let mut rows = Vec::new();
    for line in output.lines().skip(1) {
        if let Some(row) = parse_lsof_line(line) {
            rows.push(row);
        }
    }
    rows
}

#[cfg(unix)]
fn parse_lsof_line(line: &str) -> Option<ListeningPort> {
    let mut parts = line.split_whitespace();
    let process = parts.next()?.to_string();
    let pid: u32 = parts.next()?.parse().ok()?;
    for token in parts {
        if token.contains(':') && !token.starts_with("0x") {
            if let Some(port) = parse_lsof_name_port(token) {
                return Some(ListeningPort {
                    port,
                    pid,
                    process,
                    protocol: "TCP".to_string(),
                });
            }
        }
    }
    None
}

#[cfg(unix)]
fn parse_lsof_name_port(name: &str) -> Option<u16> {
    // e.g. "127.0.0.1:3000", "*:8080", "[::1]:5173"
    let after_colon = name.rsplit(':').next()?;
    let port_str = after_colon.trim_end_matches("(LISTEN)");
    port_str.parse().ok()
}

/// `iced` subscription — one scan per configured interval.
pub fn port_scanner_subscription(cfg: PortScannerConfig) -> Subscription<Message> {
    if !cfg.enabled {
        return Subscription::none();
    }
    let interval = Duration::from_secs(cfg.interval_secs.max(1));
    let key = PortScannerKey { cfg, interval };
    Subscription::run_with(key, |key: &PortScannerKey| {
        port_scanner_stream(key.cfg.clone(), key.interval)
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PortScannerKey {
    cfg: PortScannerConfig,
    interval: Duration,
}

fn port_scanner_stream(
    cfg: PortScannerConfig,
    interval: Duration,
) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(2, move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
        loop {
            let cfg = cfg.clone();
            let rows = tokio::task::spawn_blocking(move || scan_listening_ports(&cfg))
                .await
                .unwrap_or_default();
            let _ = output.send(Message::PortScannerUpdated(rows)).await;
            tokio::time::sleep(interval).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lsof_line_extracts_port_pid_process() {
        let line = "node    12345 admin   23u  IPv4 0xdeadbeef      0t0  TCP *:3000 (LISTEN)";
        let row = parse_lsof_line(line).expect("parse");
        assert_eq!(row.port, 3000);
        assert_eq!(row.pid, 12345);
        assert_eq!(row.process, "node");
    }

    #[test]
    fn parse_lsof_line_ipv6() {
        let line = "python3 99 user  4u  IPv6 0x0      0t0  TCP [::1]:5173 (LISTEN)";
        let row = parse_lsof_line(line).expect("parse");
        assert_eq!(row.port, 5173);
        assert_eq!(row.pid, 99);
    }

    #[test]
    fn min_port_filter() {
        let cfg = PortScannerConfig {
            enabled: true,
            interval_secs: 5,
            min_port: 1024,
        };
        let output = "COMMAND PID USER FD TYPE DEVICE OFF NODE NAME\n\
                      sshd 1 root 3u IPv4 0 0t0 TCP *:22 (LISTEN)\n\
                      node 2 user 4u IPv4 0 0t0 TCP *:3000 (LISTEN)\n";
        let mut rows = parse_lsof_output(output.to_string());
        rows.retain(|r| r.port >= cfg.min_port);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].port, 3000);
    }

    #[test]
    fn disabled_returns_empty() {
        let cfg = PortScannerConfig {
            enabled: false,
            ..Default::default()
        };
        assert!(scan_listening_ports(&cfg).is_empty());
    }
}
