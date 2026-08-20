//! GAP-38:SCP/SFTP 文件传输——配置 + scp 上传 MVP。

use std::path::Path;

/// `[file_transfer]` 配置。
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

/// 启用时经 `scp` 上传本地文件。MVP:调用系统 scp。
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

/// 启用时经 `scp` 下载远程文件。MVP:调用系统 scp。
#[cfg(unix)]
pub fn scp_download(
    cfg: &FileTransferConfig,
    user_host: &str,
    remote_path: &str,
    local: &Path,
) -> Result<(), String> {
    if !cfg.enabled {
        return Err("file_transfer disabled".into());
    }
    if let Some(parent) = local.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let src = format!("{user_host}:{remote_path}");
    let status = std::process::Command::new("scp")
        .arg(&src)
        .arg(local)
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

#[cfg(not(unix))]
pub fn scp_download(
    _cfg: &FileTransferConfig,
    _user_host: &str,
    _remote_path: &str,
    _local: &Path,
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

    #[test]
    fn disabled_download_returns_error() {
        let cfg = FileTransferConfig::default();
        assert!(scp_download(&cfg, "u@h", "/tmp/x", Path::new("/tmp/x")).is_err());
    }
}
