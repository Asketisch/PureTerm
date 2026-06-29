//! GAP-38: SCP/SFTP file transfer — config + scp upload MVP.

use std::path::Path;

/// `[file_transfer]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTransferConfig {
    pub enabled: bool,
    pub prefer_sftp: bool,
}

impl Default for FileTransferConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            prefer_sftp: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FileTransferState {
    pub last_error: Option<String>,
    pub in_progress: bool,
}

/// Upload a local file via `scp` when enabled. MVP: shell out to system scp.
#[cfg(unix)]
pub fn scp_upload(
    cfg: &FileTransferConfig,
    local: &Path,
    user_host: &str,
    remote_path: &str,
) -> Result<(), String> {
    if !cfg.enabled {
        return Err("file_transfer disabled".into());
    }
    if !local.exists() {
        return Err(format!("local file missing: {}", local.display()));
    }
    let dest = format!("{user_host}:{remote_path}");
    let status = std::process::Command::new("scp")
        .arg(local)
        .arg(&dest)
        .status()
        .map_err(|e| format!("scp spawn failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("scp exited with {status}"))
    }
}

#[cfg(not(unix))]
pub fn scp_upload(
    _cfg: &FileTransferConfig,
    _local: &Path,
    _user_host: &str,
    _remote_path: &str,
) -> Result<(), String> {
    Err("file transfer requires Unix".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_error() {
        let cfg = FileTransferConfig::default();
        assert!(scp_upload(&cfg, Path::new("/tmp/x"), "u@h", "/tmp/x").is_err());
    }
}
