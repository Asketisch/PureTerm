//! GAP-39:串口与 Telnet 连接 MVP。
//!
//! 可用时拉起外部 `telnet` / `nc` / `cu`;`SshProfile` 上的配置字段
//! 复用 `session_type` 判别字段。

use std::process::Command;

/// 会话传输类型(扩展 SSH 配置档)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SessionType {
    #[default]
    Ssh,
    Telnet,
    Serial,
}

impl SessionType {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "telnet" => Self::Telnet,
            "serial" => Self::Serial,
            _ => Self::Ssh,
        }
    }
}

/// 串口行参数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerialSettings {
    pub port: String,
    pub baud: u32,
    pub data_bits: u8,
    pub parity: String,
    pub stop_bits: u8,
}

impl Default for SerialSettings {
    fn default() -> Self {
        Self {
            port: "/dev/ttyUSB0".into(),
            baud: 115200,
            data_bits: 8,
            parity: "none".into(),
            stop_bits: 1,
        }
    }
}

/// 构造在 PTY 包装器中拉起 telnet/串口会话的 argv。
/// 没有合适的二进制时返回 `None`。
pub fn spawn_argv(
    kind: SessionType,
    host: &str,
    port: u16,
    serial: &SerialSettings,
) -> Option<Vec<String>> {
    match kind {
        SessionType::Telnet => {
            if which("telnet") {
                Some(vec![
                    "telnet".into(),
                    host.into(),
                    port.to_string(),
                ])
            } else if which("nc") {
                Some(vec!["nc".into(), host.into(), port.to_string()])
            } else {
                None
            }
        }
        SessionType::Serial => {
            if which("cu") {
                Some(vec![
                    "cu".into(),
                    "-l".into(),
                    serial.port.clone(),
                    "-s".into(),
                    serial.baud.to_string(),
                ])
            } else if which("screen") {
                Some(vec![
                    "screen".into(),
                    serial.port.clone(),
                    serial.baud.to_string(),
                ])
            } else {
                None
            }
        }
        SessionType::Ssh => None,
    }
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telnet_prefers_telnet_binary() {
        let argv = spawn_argv(SessionType::Telnet, "example.com", 23, &SerialSettings::default());
        if which("telnet") {
            assert_eq!(argv.unwrap()[0], "telnet");
        }
    }
}
