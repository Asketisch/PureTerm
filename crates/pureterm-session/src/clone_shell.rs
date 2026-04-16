//! Clone shell — duplicate an existing session's environment.

use anyhow::Result;

/// Information needed to clone a session's environment.
#[derive(Debug, Clone)]
pub struct CloneInfo {
    /// Session type (local, ssh, serial, telnet).
    pub session_type: String,
    /// Working directory.
    pub cwd: Option<String>,
    /// Environment variables.
    pub env: std::collections::HashMap<String, String>,
    /// Connection details (host, port, etc.) for remote sessions.
    pub connection_params: Option<ConnectionCloneParams>,
}

#[derive(Debug, Clone)]
pub enum ConnectionCloneParams {
    Ssh {
        host: String,
        port: u16,
        user: String,
    },
    Telnet {
        host: String,
        port: u16,
    },
    Serial {
        path: String,
        baud_rate: u32,
    },
}

/// Manages cloning of terminal sessions.
pub struct CloneShellManager;

impl CloneShellManager {
    /// Extract clone info from a running session by examining its environment.
    /// For local sessions, this reads /proc/self/environ (Linux) or uses `env` output.
    pub fn extract_env_from_local() -> Result<CloneInfo> {
        let cwd = std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string());

        let env: std::collections::HashMap<String, String> = std::env::vars().collect();

        Ok(CloneInfo {
            session_type: "local".to_string(),
            cwd,
            env,
            connection_params: None,
        })
    }

    /// Build the shell command that replicates a session's environment.
    pub fn build_clone_command(info: &CloneInfo) -> String {
        let mut cmd = String::new();

        // Set environment variables
        for (key, value) in &info.env {
            // Skip some vars that should be inherited naturally
            if key.starts_with("SSH_") || key == "TERM" || key == "SHELL" {
                continue;
            }
            cmd.push_str(&format!("export {}={:?} && ", key, value));
        }

        // Change to working directory
        if let Some(ref cwd) = info.cwd {
            cmd.push_str(&format!("cd {:?} && ", cwd));
        }

        // Start a new shell
        cmd.push_str("exec $SHELL");
        cmd
    }
}
