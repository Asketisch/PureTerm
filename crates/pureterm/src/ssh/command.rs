use std::path::PathBuf;

/// SSH command builder for ControlMaster connections
pub struct SshCommandBuilder {
    socket_path: PathBuf,
    host: String,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
}

impl SshCommandBuilder {
    pub fn new(socket_path: PathBuf, host: String) -> Self {
        Self {
            socket_path,
            host,
            user: None,
            port: None,
            identity_file: None,
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