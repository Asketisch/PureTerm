//! PureTerm 的单实例强制执行。
//!
//! 确保一次只运行一个 PureTerm 实例。
//! 如果第二个实例启动,它通过 IPC 将其参数发送到
//! 运行中的实例并退出。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// 单实例 IPC 的默认端口。
const SINGLE_INSTANCE_PORT: u16 = 19538;

/// 管理单实例强制执行。
pub struct SingleInstance {
    /// 用于 IPC 的 TCP 监听器(保持端口绑定)。
    listener: Option<TcpListener>,
    port: u16,
}

impl SingleInstance {
    pub fn new() -> Self {
        Self {
            listener: None,
            port: SINGLE_INSTANCE_PORT,
        }
    }

    /// 尝试获取单实例锁。
    /// 如果是第一个实例则返回 true。
    pub fn try_claim(&mut self) -> bool {
        let addr: SocketAddr = format!("127.0.0.1:{}", self.port).parse().unwrap();

        match TcpListener::bind(addr) {
            Ok(listener) => {
                listener.set_nonblocking(true).ok();
                self.listener = Some(listener);
                log::info!("Single instance lock acquired on port {}", self.port);
                true
            }
            Err(_) => {
                log::info!("Another instance is already running on port {}", self.port);
                false
            }
        }
    }

    /// 将参数发送到运行中的实例。
    pub fn send_to_running(&self, args: &[String]) -> bool {
        let addr = format!("127.0.0.1:{}", self.port);
        if let Ok(mut stream) = TcpStream::connect_timeout(
            &addr.parse().unwrap(),
            Duration::from_secs(2),
        ) {
            let message = args.join("\0");
            stream.write_all(message.as_bytes()).is_ok()
        } else {
            false
        }
    }

    /// 检查来自其他实例的消息(非阻塞)。
    /// 返回由其他实例发送的参数。
    pub fn check_incoming(&self) -> Option<Vec<String>> {
        let listener = self.listener.as_ref()?;
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(_) => return None,
        };

        let mut buf = Vec::new();
        stream.set_read_timeout(Some(Duration::from_secs(1))).ok();
        stream.read_to_end(&mut buf).ok()?;

        let message = String::from_utf8_lossy(&buf);
        let args: Vec<String> = message.split('\0').map(String::from).collect();

        if args.is_empty() || (args.len() == 1 && args[0].is_empty()) {
            // 只是 ping — 将窗口带到前台
            Some(vec!["--bring-to-front".to_string()])
        } else {
            Some(args)
        }
    }

    /// 释放单实例锁。
    pub fn release(&mut self) {
        self.listener = None;
        log::info!("Single instance lock released");
    }
}

impl Default for SingleInstance {
    fn default() -> Self {
        Self::new()
    }
}
