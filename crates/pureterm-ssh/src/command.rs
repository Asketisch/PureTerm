use std::path::PathBuf;

/// SSH command builder for ControlMaster connections
pub struct SshCommandBuilder {
    socket_path: PathBuf,
    host: String,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
    forward_agent: bool,
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
}