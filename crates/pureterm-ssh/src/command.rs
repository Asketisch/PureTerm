use std::path::PathBuf;

/// GAP-40: port-forward / X11 flags plumbed into `ssh -tt` (`-L`/`-R`/`-X`/`-Y`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SshForwards {
    /// Each entry is passed verbatim to `ssh -L` (e.g. `8080:localhost:80`).
    pub local: Vec<String>,
    /// Each entry is passed verbatim to `ssh -R`.
    pub remote: Vec<String>,
    /// Request untrusted X11 forwarding (`-X`).
    pub x11: bool,
    /// Request trusted X11 forwarding (`-Y`; takes precedence over `x11`).
    pub x11_trusted: bool,
}

impl SshForwards {
    /// One-line summary for status-bar / dialog hints (e.g. `L2 R1 X`).
    pub fn summary(&self) -> Option<String> {
        let mut parts = Vec::new();
        if !self.local.is_empty() {
            parts.push(format!("L{}", self.local.len()));
        }
        if !self.remote.is_empty() {
            parts.push(format!("R{}", self.remote.len()));
        }
        if self.x11_trusted {
            parts.push("Y".into());
        } else if self.x11 {
            parts.push("X".into());
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }
}

/// SSH command builder for ControlMaster connections
pub struct SshCommandBuilder {
    socket_path: PathBuf,
    host: String,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
    forward_agent: bool,
    forwards: SshForwards,
    /// GAP-41: ProxyJump chain (bastion hosts).
    jump_hosts: Vec<String>,
}

impl SshCommandBuilder {
    pub fn new(socket_path: PathBuf, host: String) -> Self {
        Self {
            socket_path,
            host,
            user: None,
            port: None,
            identity_file: None,
            forward_agent: false,
            forwards: SshForwards::default(),
            jump_hosts: Vec::new(),
        }
    }

    pub fn with_user(mut self, user: &str) -> Self {
        self.user = Some(user.to_string());
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn with_identity_file(mut self, path: &str) -> Self {
        self.identity_file = Some(path.to_string());
        self
    }

    /// GAP-5.4: request `-o ForwardAgent=yes` so the local ssh-agent is
    /// forwarded to the remote host (enables passwordless remote `git push`
    /// etc. after `ssh-add`). Default off (trust risk on untrusted hosts).
    pub fn with_forward_agent(mut self, on: bool) -> Self {
        self.forward_agent = on;
        self
    }

    /// GAP-40: local/remote port forwards and X11 forwarding.
    pub fn with_forwards(mut self, forwards: SshForwards) -> Self {
        self.forwards = forwards;
        self
    }

    /// GAP-41: `-J host1,host2` jump host chain.
    pub fn with_jump_hosts(mut self, hosts: Vec<String>) -> Self {
        self.jump_hosts = hosts;
        self
    }

    pub fn build(&self) -> Vec<String> {
        let mut cmd = vec![
            "ssh".to_string(),
            "-tt".to_string(),
            "-S".to_string(),
            self.socket_path.display().to_string(),
            "-o".to_string(),
            "ControlMaster=yes".to_string(),
            "-o".to_string(),
            "ControlPersist=600".to_string(),
            "-o".to_string(),
            "ExitOnForwardFailure=yes".to_string(),
            // Bound connection timeout so unreachable hosts fail within 10s
            // instead of hanging indefinitely (the exit handler grace period
            // will catch and surface the error).
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
        ];

        // GAP-5.4: forward the local ssh-agent to the remote host when asked.
        if self.forward_agent {
            cmd.push("-o".to_string());
            cmd.push("ForwardAgent=yes".to_string());
        }

        // GAP-41: ProxyJump chain.
        if !self.jump_hosts.is_empty() {
            cmd.push("-J".to_string());
            cmd.push(self.jump_hosts.join(","));
        }

        // GAP-40: port forwards + X11 (dynamic `-D` remains GAP-5.2 `ssh tunnel`).
        for spec in &self.forwards.local {
            cmd.push("-L".to_string());
            cmd.push(spec.clone());
        }
        for spec in &self.forwards.remote {
            cmd.push("-R".to_string());
            cmd.push(spec.clone());
        }
        if self.forwards.x11_trusted {
            cmd.push("-Y".to_string());
        } else if self.forwards.x11 {
            cmd.push("-X".to_string());
        }

        if let Some(ref user) = self.user {
            cmd.push("-l".to_string());
            cmd.push(user.clone());
        }

        if let Some(port) = self.port {
            cmd.push("-p".to_string());
            cmd.push(port.to_string());
        }

        if let Some(ref key) = self.identity_file {
            cmd.push("-i".to_string());
            cmd.push(key.clone());
        }

        cmd.push(self.host.clone());
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder() -> SshCommandBuilder {
        SshCommandBuilder::new(PathBuf::from("/tmp/socket"), "host".to_string())
    }

    #[test]
    fn forward_agent_off_by_default() {
        let cmd = builder().build();
        assert!(!cmd.iter().any(|a| a == "ForwardAgent=yes"));
    }

    #[test]
    fn forward_agent_on_emits_option() {
        let cmd = builder().with_forward_agent(true).build();
        let i = cmd
            .iter()
            .position(|a| a == "ForwardAgent=yes")
            .expect("ForwardAgent=yes should be present");
        // Must be preceded by `-o`.
        assert_eq!(cmd[i - 1], "-o");
    }

    #[test]
    fn forward_agent_explicit_false_omits_option() {
        let cmd = builder().with_forward_agent(false).build();
        assert!(!cmd.iter().any(|a| a == "ForwardAgent=yes"));
    }

    #[test]
    fn forwards_emit_l_r_x_y() {
        let cmd = builder()
            .with_forwards(SshForwards {
                local: vec!["8080:localhost:80".into()],
                remote: vec!["9000:127.0.0.1:22".into()],
                x11: true,
                x11_trusted: false,
            })
            .build();
        assert!(cmd.windows(2).any(|w| w == ["-L", "8080:localhost:80"]));
        assert!(cmd.windows(2).any(|w| w == ["-R", "9000:127.0.0.1:22"]));
        assert!(cmd.iter().any(|a| a == "-X"));
        assert!(!cmd.iter().any(|a| a == "-Y"));
    }

    #[test]
    fn x11_trusted_prefers_y_over_x() {
        let cmd = builder()
            .with_forwards(SshForwards {
                x11: true,
                x11_trusted: true,
                ..Default::default()
            })
            .build();
        assert!(cmd.iter().any(|a| a == "-Y"));
        assert!(!cmd.iter().any(|a| a == "-X"));
    }

    #[test]
    fn forward_summary_counts() {
        let s = SshForwards {
            local: vec!["a".into(), "b".into()],
            remote: vec!["c".into()],
            x11: true,
            ..Default::default()
        };
        assert_eq!(s.summary().as_deref(), Some("L2 R1 X"));
    }
}