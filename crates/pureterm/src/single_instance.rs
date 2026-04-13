//! Single instance enforcement for PureTerm.
//!
//! Ensures only one instance of PureTerm runs at a time.
//! If a second instance starts, it sends its arguments to the
//! running instance via IPC and exits.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Default port for single-instance IPC.
const SINGLE_INSTANCE_PORT: u16 = 19538;

/// Manages single instance enforcement.
pub struct SingleInstance {
    /// The TCP listener for IPC (held to keep the port bound).
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

    /// Try to claim the single instance lock.
    /// Returns true if this is the first instance.
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

    /// Send arguments to the running instance.
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

    /// Check for messages from other instances (non-blocking).
    /// Returns the arguments sent by the other instance.
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
            // Just a ping — bring window to front
            Some(vec!["--bring-to-front".to_string()])
        } else {
            Some(args)
        }
    }

    /// Release the single instance lock.
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
