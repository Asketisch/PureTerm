use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::command::SshCommandBuilder;
use super::pty::SshPty;

/// Key for connection pooling (unique per user@host:port)
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ConnectionKey {
    pub host: String,
    pub port: u16,
    pub user: String,
}

/// Entry in the connection pool
struct ConnectionEntry {
    socket_path: PathBuf,
    channel_count: usize,
    last_activity: Instant,
}

/// Manages SSH connections with ControlMaster socket pooling
pub struct SshSessionManager {
    pool: HashMap<ConnectionKey, ConnectionEntry>,
    socket_dir: PathBuf,
    max_idle_time: Duration,
}

impl SshSessionManager {
    pub fn new(socket_dir: PathBuf) -> Self {
        Self {
            pool: HashMap::new(),
            socket_dir,
            max_idle_time: Duration::from_secs(30 * 60), // 30 minutes
        }
    }

    fn socket_path(&self, key: &ConnectionKey) -> PathBuf {
        self.socket_dir
            .join(format!("{}@{}:{}", key.user, key.host, key.port))
    }

    pub fn get_or_create(&mut self, host: &str, user: Option<&str>, port: Option<u16>) -> PathBuf {
        let key = ConnectionKey {
            host: host.to_string(),
            port: port.unwrap_or(22),
            user: user.unwrap_or("").to_string(),
        };

        if let Some(entry) = self.pool.get_mut(&key) {
            entry.channel_count += 1;
            entry.last_activity = Instant::now();
            return entry.socket_path.clone();
        }

        let socket = self.socket_path(&key);
        self.pool.insert(
            key,
            ConnectionEntry {
                socket_path: socket.clone(),
                channel_count: 1,
                last_activity: Instant::now(),
            },
        );
        socket
    }

    pub fn release(&mut self, host: &str, user: Option<&str>, port: Option<u16>) {
        let key = ConnectionKey {
            host: host.to_string(),
            port: port.unwrap_or(22),
            user: user.unwrap_or("").to_string(),
        };

        if let Some(entry) = self.pool.get_mut(&key) {
            entry.channel_count = entry.channel_count.saturating_sub(1);
            entry.last_activity = Instant::now();

            // If channel_count is 0, clean up the ControlMaster socket
            if entry.channel_count == 0 {
                let socket_path = entry.socket_path.clone();
                // Remove from pool first
                self.pool.remove(&key);

                // Send ssh -O exit to clean up the ControlMaster socket
                let mut cmd = std::process::Command::new("ssh");
                cmd.arg("-O")
                    .arg("exit")
                    .arg("-o")
                    .arg(format!("ControlPath={}", socket_path.display()));
                if let Err(e) = cmd.output() {
                    log::debug!("Failed to exit ControlMaster socket: {}", e);
                }

                // Also try to remove the socket file if it still exists
                if socket_path.exists() {
                    let _ = std::fs::remove_file(&socket_path);
                }
            }
        }
    }

    pub fn cleanup_idle(&mut self) {
        let now = Instant::now();
        self.pool.retain(|_, entry| {
            entry.channel_count > 0 || now.duration_since(entry.last_activity) < self.max_idle_time
        });
    }

    /// Spawn a new PTY for the given SSH connection parameters.
    ///
    /// Uses ControlMaster socket reuse if available, spawning `ssh -tt` with
    /// the configured identity file and options.
    pub fn spawn_pty(
        &mut self,
        host: &str,
        user: Option<&str>,
        port: Option<u16>,
        window_size: Option<(u16, u16)>,
    ) -> std::io::Result<SshPty> {
        let socket_path = self.get_or_create(host, user, port);

        // Build SSH command with ControlMaster.
        let mut builder = SshCommandBuilder::new(socket_path, host.to_string());
        if let Some(u) = user {
            builder = builder.with_user(u);
        }
        if let Some(p) = port {
            builder = builder.with_port(p);
        }

        let cmd = builder.build();
        SshPty::spawn(&cmd, window_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_key_equality() {
        let key1 = ConnectionKey {
            host: "server".into(),
            port: 22,
            user: "user".into(),
        };
        let key2 = ConnectionKey {
            host: "server".into(),
            port: 22,
            user: "user".into(),
        };
        let key3 = ConnectionKey {
            host: "other".into(),
            port: 22,
            user: "user".into(),
        };
        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_session_manager_pooling() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let mut manager = SshSessionManager::new(temp_dir.path().to_path_buf());

        // First call creates a new entry
        let socket1 = manager.get_or_create("example.com", Some("admin"), Some(22));
        assert!(socket1.to_string_lossy().contains("admin@example.com:22"));

        // Second call with same params returns same socket
        let socket2 = manager.get_or_create("example.com", Some("admin"), Some(22));
        assert_eq!(socket1, socket2);

        // Different params create different entries
        let socket3 = manager.get_or_create("example.com", Some("root"), Some(22));
        assert_ne!(socket1, socket3);

        let socket4 = manager.get_or_create("other.com", Some("admin"), Some(22));
        assert_ne!(socket1, socket4);
    }

    #[test]
    fn test_session_manager_release() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let mut manager = SshSessionManager::new(temp_dir.path().to_path_buf());

        let socket = manager.get_or_create("example.com", Some("admin"), Some(22));
        manager.release("example.com", Some("admin"), Some(22));

        // After release, a new get_or_create should return same socket (pooled)
        let socket2 = manager.get_or_create("example.com", Some("admin"), Some(22));
        assert_eq!(socket, socket2);
    }
}