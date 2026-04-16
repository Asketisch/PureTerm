//! SSH jump host (bastion) support.

use std::collections::HashMap;

/// Configuration for connecting through a jump host (bastion/proxy host).
#[derive(Debug, Clone)]
pub struct JumpHostConfig {
    /// Hostname of the jump host.
    pub host: String,
    /// Port of the jump host SSH server.
    pub port: u16,
    /// Username for the jump host.
    pub user: String,
    /// Ordered list of jump hosts (for multi-hop).
    pub hops: Vec<JumpHop>,
}

#[derive(Debug, Clone)]
pub struct JumpHop {
    pub host: String,
    pub port: u16,
    pub user: String,
}

/// Manages connections through jump hosts.
pub struct JumpHostManager {
    configs: HashMap<String, JumpHostConfig>,
}

impl JumpHostManager {
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }

    /// Register a jump host configuration for a target host pattern.
    pub fn register(&mut self, target_pattern: String, config: JumpHostConfig) {
        self.configs.insert(target_pattern, config);
    }

    /// Find the jump host configuration for a target host.
    pub fn find_for_host(&self, target_host: &str) -> Option<&JumpHostConfig> {
        // Exact match first
        if let Some(config) = self.configs.get(target_host) {
            return Some(config);
        }
        // Then try suffix match (*.example.com)
        for (pattern, config) in &self.configs {
            if pattern.starts_with("*.") {
                let domain = &pattern[1..]; // e.g. ".example.com"
                if target_host.ends_with(domain) {
                    return Some(config);
                }
            }
        }
        None
    }

    /// Get the ProxyCommand-style connection string for a jump host.
    pub fn proxy_command(config: &JumpHostConfig) -> String {
        if config.hops.is_empty() {
            format!(
                "ssh -W %h:%p -p {} {}@{}",
                config.port, config.user, config.host
            )
        } else {
            let mut cmd = String::from("ssh");
            for hop in &config.hops {
                cmd.push_str(&format!(" -J {}@{}:{}", hop.user, hop.host, hop.port));
            }
            cmd.push_str(&format!(" -W %h:%p -p {} {}@{}", config.port, config.user, config.host));
            cmd
        }
    }

    /// List all registered jump host configurations.
    pub fn list_configs(&self) -> &HashMap<String, JumpHostConfig> {
        &self.configs
    }

    /// Remove a jump host configuration.
    pub fn remove(&mut self, target_pattern: &str) {
        self.configs.remove(target_pattern);
    }
}

impl Default for JumpHostManager {
    fn default() -> Self {
        Self::new()
    }
}
