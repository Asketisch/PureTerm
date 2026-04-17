//! SSH 会话的 HTTP/SOCKS5 代理支持。

use std::net::SocketAddr;

/// 会话的代理配置。
#[derive(Debug, Clone)]
pub enum ProxyConfig {
    /// 无代理。
    None,
    /// HTTP CONNECT 代理。
    Http {
        addr: SocketAddr,
        auth: Option<ProxyAuth>,
    },
    /// SOCKS5 代理。
    Socks5 {
        addr: SocketAddr,
        auth: Option<ProxyAuth>,
    },
}

/// 代理认证凭据。
#[derive(Debug, Clone)]
pub struct ProxyAuth {
    pub username: String,
    pub password: String,
}

impl ProxyConfig {
    /// 将代理 URL 解析为 ProxyConfig。
    /// 支持: http://host:port, socks5://host:port, 可选 user:pass@。
    pub fn from_url(url: &str) -> Option<Self> {
        if url.starts_with("http://") {
            Self::parse_http(url)
        } else if url.starts_with("socks5://") {
            Self::parse_socks5(url)
        } else {
            None
        }
    }

    fn parse_http(url: &str) -> Option<Self> {
        let rest = url.trim_start_matches("http://");
        let (auth_part, host_part) = if let Some(at_pos) = rest.find('@') {
            let (userpass, host) = rest.split_at(at_pos);
            (Some(userpass), host.trim_start_matches('@'))
        } else {
            (None, rest)
        };

        let auth = auth_part.and_then(|up| {
            let mut parts = up.splitn(2, ':');
            let user = parts.next()?.to_string();
            let pass = parts.next().unwrap_or("").to_string();
            Some(ProxyAuth { username: user, password: pass })
        });

        let addr: SocketAddr = host_part.parse().ok()?;
        Some(ProxyConfig::Http { addr, auth })
    }

    fn parse_socks5(url: &str) -> Option<Self> {
        let rest = url.trim_start_matches("socks5://");
        let (auth_part, host_part) = if let Some(at_pos) = rest.find('@') {
            let (userpass, host) = rest.split_at(at_pos);
            (Some(userpass), host.trim_start_matches('@'))
        } else {
            (None, rest)
        };

        let auth = auth_part.and_then(|up| {
            let mut parts = up.splitn(2, ':');
            let user = parts.next()?.to_string();
            let pass = parts.next().unwrap_or("").to_string();
            Some(ProxyAuth { username: user, password: pass })
        });

        let addr: SocketAddr = host_part.parse().ok()?;
        Some(ProxyConfig::Socks5 { addr, auth })
    }

    pub fn is_none(&self) -> bool {
        matches!(self, ProxyConfig::None)
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        ProxyConfig::None
    }
}

/// 通过代理的 TCP 连接。
pub struct ProxyConnection;

impl ProxyConnection {
    /// 通过 HTTP CONNECT 代理连接到目标。
    pub async fn connect_http(
        proxy: SocketAddr,
        target: &str,
        port: u16,
        auth: Option<&ProxyAuth>,
    ) -> std::io::Result<tokio::net::TcpStream> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(proxy).await?;

        let connect_request = if let Some(auth) = auth {
            let credentials = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                format!("{}:{}", auth.username, auth.password),
            );
            format!(
                "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\nProxy-Authorization: Basic {}\r\n\r\n",
                target, port, target, port, credentials
            )
        } else {
            format!(
                "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n\r\n",
                target, port, target, port
            )
        };

        stream.write_all(connect_request.as_bytes()).await?;

        let mut response = vec![0u8; 4096];
        let n = stream.read(&mut response).await?;
        let response_str = String::from_utf8_lossy(&response[..n]);
        if response_str.contains("200") {
            Ok(stream)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("Proxy rejected connection: {}", response_str.lines().next().unwrap_or("")),
            ))
        }
    }

    /// 通过 SOCKS5 代理连接到目标。
    pub async fn connect_socks5(
        proxy: SocketAddr,
        target: &str,
        port: u16,
        auth: Option<&ProxyAuth>,
    ) -> std::io::Result<tokio::net::TcpStream> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(proxy).await?;

        // SOCKS5 问候
        let greeting = if auth.is_some() {
            vec![0x05, 0x02, 0x00, 0x02] // version 5, 2 methods: no-auth, username/password
        } else {
            vec![0x05, 0x01, 0x00] // version 5, 1 method: no-auth
        };
        stream.write_all(&greeting).await?;

        let mut buf = [0u8; 2];
        stream.read_exact(&mut buf).await?;
        if buf[0] != 0x05 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Not SOCKS5"));
        }

        // 需要时进行认证
        if buf[1] == 0x02 {
            let auth = auth.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Proxy requires auth")
            })?;
            let mut auth_msg = vec![0x01];
            auth_msg.push(auth.username.len() as u8);
            auth_msg.extend_from_slice(auth.username.as_bytes());
            auth_msg.push(auth.password.len() as u8);
            auth_msg.extend_from_slice(auth.password.as_bytes());
            stream.write_all(&auth_msg).await?;
            let mut auth_resp = [0u8; 2];
            stream.read_exact(&mut auth_resp).await?;
            if auth_resp[1] != 0x00 {
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Proxy auth failed"));
            }
        }

        // 连接请求
        let target_bytes = target.as_bytes();
        let mut connect_msg = vec![
            0x05, 0x01, 0x00, 0x03, // version, connect, reserved, domain
            target_bytes.len() as u8,
        ];
        connect_msg.extend_from_slice(target_bytes);
        connect_msg.extend_from_slice(&(port as u16).to_be_bytes());
        stream.write_all(&connect_msg).await?;

        let mut resp = [0u8; 10]; // min response size
        stream.read_exact(&mut resp).await?;
        if resp[1] != 0x00 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("SOCKS5 connect failed: status {}", resp[1]),
            ));
        }

        Ok(stream)
    }
}
