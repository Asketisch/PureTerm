use std::collections::HashMap;

use anyhow::{anyhow, Result};

/// Key for identifying connections.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ConnectionKey {
    pub host: String,
    pub port: u16,
    pub user: String,
}

/// Statistics about a shared connection.
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub key: ConnectionKey,
    pub channel_count: usize,
    pub created_at: std::time::Instant,
    pub last_activity: std::time::Instant,
}

struct ConnectionEntry {
    stats: ConnectionStats,
}

/// Manages shared SSH connections.
///
/// The multiplexer tracks connection metadata so that multiple tabs/sessions
/// can reuse the same underlying SSH transport. Actual russh `Handle` storage
/// and channel operations are handled by `SshSession`; this struct is
/// responsible only for pooling logic.
pub struct SshMultiplexer {
    connections: HashMap<ConnectionKey, ConnectionEntry>,
    max_connections: usize,
    max_idle_time: std::time::Duration,
}

impl SshMultiplexer {
    /// Create a new multiplexer with defaults (max 10 connections, 30 min idle timeout).
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            max_connections: 10,
            max_idle_time: std::time::Duration::from_secs(30 * 60),
        }
    }

    /// Register a new connection.
    ///
    /// If at max capacity, the most idle connection is evicted first.
    /// Returns the `ConnectionKey` for the registered connection.
    pub fn register_connection(&mut self, host: &str, port: u16, user: &str) -> ConnectionKey {
        let key = ConnectionKey {
            host: host.to_string(),
            port,
            user: user.to_string(),
        };

        // If this key already exists, just bump channel count and update activity.
        if self.connections.contains_key(&key) {
            let entry = self.connections.get_mut(&key).unwrap();
            entry.stats.channel_count += 1;
            entry.stats.last_activity = std::time::Instant::now();
            return key;
        }

        // Evict the most idle connection if we are at capacity.
        if self.connections.len() >= self.max_connections {
            self.evict_most_idle();
        }

        let now = std::time::Instant::now();
        self.connections.insert(
            key.clone(),
            ConnectionEntry {
                stats: ConnectionStats {
                    key: key.clone(),
                    channel_count: 1,
                    created_at: now,
                    last_activity: now,
                },
            },
        );

        key
    }

    /// Release a connection.
    ///
    /// Decrements the channel count. If zero channels remain, the connection
    /// entry is removed.
    pub fn release_connection(&mut self, key: &ConnectionKey) {
        if let Some(entry) = self.connections.get_mut(key) {
            entry.stats.channel_count = entry.stats.channel_count.saturating_sub(1);
            entry.stats.last_activity = std::time::Instant::now();
            if entry.stats.channel_count == 0 {
                self.connections.remove(key);
            }
        }
    }

    /// Check if a connection exists and return its stats.
    pub fn get_connection(&self, key: &ConnectionKey) -> Option<&ConnectionStats> {
        self.connections.get(key).map(|e| &e.stats)
    }

    /// List all active connections with stats.
    pub fn active_connections(&self) -> Vec<ConnectionStats> {
        self.connections.values().map(|e| e.stats.clone()).collect()
    }

    /// Remove all connections.
    pub fn close_all(&mut self) {
        self.connections.clear();
    }

    /// Remove connections idle beyond `max_idle_time`.
    ///
    /// Returns the count of removed connections.
    pub fn cleanup_idle(&mut self) -> usize {
        let now = std::time::Instant::now();
        let to_remove: Vec<ConnectionKey> = self
            .connections
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.stats.last_activity) > self.max_idle_time)
            .map(|(k, _)| k.clone())
            .collect();

        let count = to_remove.len();
        for key in to_remove {
            self.connections.remove(&key);
        }
        count
    }

    /// Number of tracked connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Sum of all channel counts across connections.
    pub fn total_channels(&self) -> usize {
        self.connections.values().map(|e| e.stats.channel_count).sum()
    }

    /// Increment the channel count for an existing connection.
    pub fn increment_channels(&mut self, key: &ConnectionKey) -> Result<()> {
        let entry = self
            .connections
            .get_mut(key)
            .ok_or_else(|| anyhow!("Connection not found for key {:?}", key))?;
        entry.stats.channel_count += 1;
        entry.stats.last_activity = std::time::Instant::now();
        Ok(())
    }

    /// Decrement the channel count for an existing connection.
    ///
    /// Automatically removes the entry if the count reaches zero.
    pub fn decrement_channels(&mut self, key: &ConnectionKey) -> Result<()> {
        let entry = self
            .connections
            .get_mut(key)
            .ok_or_else(|| anyhow!("Connection not found for key {:?}", key))?;
        entry.stats.channel_count = entry.stats.channel_count.saturating_sub(1);
        entry.stats.last_activity = std::time::Instant::now();
        if entry.stats.channel_count == 0 {
            self.connections.remove(key);
        }
        Ok(())
    }

    // -- helpers --

    /// Remove the connection with the oldest `last_activity`.
    fn evict_most_idle(&mut self) {
        let oldest = self
            .connections
            .iter()
            .min_by_key(|(_, e)| e.stats.last_activity)
            .map(|(k, _)| k.clone());

        if let Some(key) = oldest {
            self.connections.remove(&key);
        }
    }
}

impl Default for SshMultiplexer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_get() {
        let mut mux = SshMultiplexer::new();
        let key = mux.register_connection("host1", 22, "user");
        assert_eq!(mux.connection_count(), 1);
        assert_eq!(mux.total_channels(), 1);

        let stats = mux.get_connection(&key).unwrap();
        assert_eq!(stats.channel_count, 1);
        assert_eq!(stats.key.host, "host1");
    }

    #[test]
    fn register_same_key_increments_channels() {
        let mut mux = SshMultiplexer::new();
        let _key = mux.register_connection("host1", 22, "user");
        mux.register_connection("host1", 22, "user");
        assert_eq!(mux.connection_count(), 1);
        assert_eq!(mux.total_channels(), 2);
    }

    #[test]
    fn release_removes_when_zero_channels() {
        let mut mux = SshMultiplexer::new();
        let key = mux.register_connection("host1", 22, "user");
        assert_eq!(mux.connection_count(), 1);
        mux.release_connection(&key);
        assert_eq!(mux.connection_count(), 0);
        assert!(mux.get_connection(&key).is_none());
    }

    #[test]
    fn release_decrements_but_keeps() {
        let mut mux = SshMultiplexer::new();
        let key = mux.register_connection("host1", 22, "user");
        mux.register_connection("host1", 22, "user"); // channel_count = 2
        mux.release_connection(&key); // channel_count = 1
        assert_eq!(mux.connection_count(), 1);
        assert_eq!(mux.total_channels(), 1);
    }

    #[test]
    fn increment_decrement_channels() {
        let mut mux = SshMultiplexer::new();
        let key = mux.register_connection("host1", 22, "user");
        mux.increment_channels(&key).unwrap();
        assert_eq!(mux.total_channels(), 2);
        mux.decrement_channels(&key).unwrap();
        assert_eq!(mux.total_channels(), 1);
        mux.decrement_channels(&key).unwrap(); // auto-remove
        assert_eq!(mux.connection_count(), 0);
    }

    #[test]
    fn increment_missing_key_fails() {
        let mut mux = SshMultiplexer::new();
        let key = ConnectionKey {
            host: "nope".into(),
            port: 22,
            user: "x".into(),
        };
        assert!(mux.increment_channels(&key).is_err());
    }

    #[test]
    fn close_all() {
        let mut mux = SshMultiplexer::new();
        mux.register_connection("a", 22, "u");
        mux.register_connection("b", 22, "u");
        mux.close_all();
        assert_eq!(mux.connection_count(), 0);
    }

    #[test]
    fn evicts_at_capacity() {
        let mut mux = SshMultiplexer::new();
        mux.max_connections = 2;

        let k1 = mux.register_connection("a", 22, "u");
        let _k2 = mux.register_connection("b", 22, "u");
        // Capacity is 2, registering a third should evict the most idle (k1).
        mux.register_connection("c", 22, "u");
        assert_eq!(mux.connection_count(), 2);
        assert!(mux.get_connection(&k1).is_none());
    }

    #[test]
    fn active_connections_returns_all() {
        let mut mux = SshMultiplexer::new();
        mux.register_connection("a", 22, "u");
        mux.register_connection("b", 22, "u");
        let active = mux.active_connections();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn cleanup_idle_removes_old() {
        let mut mux = SshMultiplexer::new();
        mux.max_idle_time = std::time::Duration::from_millis(1);
        mux.register_connection("a", 22, "u");
        // Sleep briefly so the entry exceeds the idle threshold.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let removed = mux.cleanup_idle();
        assert_eq!(removed, 1);
        assert_eq!(mux.connection_count(), 0);
    }

    #[test]
    fn default_trait() {
        let mux = SshMultiplexer::default();
        assert_eq!(mux.connection_count(), 0);
        assert_eq!(mux.max_connections, 10);
    }
}
