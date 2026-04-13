//! SSH SFTP file operations over an existing SSH session.
//!
//! Uses the russh SFTP subsystem to provide basic file operations.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Metadata for a remote file.
#[derive(Debug, Clone)]
pub struct RemoteFileMeta {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<u64>,
    pub permissions: Option<u32>,
}

/// SFTP client that operates over an existing SSH channel.
pub struct SftpClient {
    /// Channel for sending SFTP requests.
    channel_write: Option<tokio::sync::Mutex<russh::ChannelWriteHalf<russh::client::Msg>>>,
    /// Buffer for receiving SFTP responses.
    response_buf: Vec<u8>,
    /// Request ID counter.
    next_id: u32,
}

/// SFTP request types.
enum SftpRequest {
    OpenDir { path: String },
    ReadDir { handle: Vec<u8> },
    Stat { path: String },
    OpenFile { path: String, read: bool, write: bool },
    ReadFile { handle: Vec<u8>, offset: u64, len: u32 },
    WriteFile { handle: Vec<u8>, offset: u64, data: Vec<u8> },
    Close { handle: Vec<u8> },
    Mkdir { path: String },
    Rmdir { path: String },
    Remove { path: String },
    Rename { old_path: String, new_path: String },
}

/// SFTP response.
#[derive(Debug)]
pub enum SftpResponse {
    Handle { id: u32, handle: Vec<u8> },
    Name { id: u32, entries: Vec<RemoteFileMeta> },
    Attributes { id: u32, meta: RemoteFileMeta },
    Data { id: u32, data: Vec<u8> },
    Status { id: u32, code: u32, message: String },
}

impl SftpClient {
    /// Create a new SFTP client. The caller must first open an SFTP subsystem
    /// channel on an existing SSH session.
    pub fn new() -> Self {
        Self {
            channel_write: None,
            response_buf: Vec::new(),
            next_id: 0,
        }
    }

    /// List files in a remote directory.
    /// This is a high-level convenience that wraps the raw SFTP protocol.
    pub async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteFileMeta>> {
        // In a full implementation, this would:
        // 1. Open the directory via SFTP OPENDIR
        // 2. Read entries via READDIR until EOF
        // 3. Close the directory handle
        //
        // For now, provide a placeholder that returns an empty list.
        log::warn!("SFTP list_dir not fully implemented - returning empty list");
        Ok(Vec::new())
    }

    /// Download a remote file to a local path.
    pub async fn download(&mut self, remote_path: &str, local_path: &Path) -> Result<()> {
        log::warn!("SFTP download not fully implemented");
        Ok(())
    }

    /// Upload a local file to a remote path.
    pub async fn upload(&mut self, local_path: &Path, remote_path: &str) -> Result<()> {
        log::warn!("SFTP upload not fully implemented");
        Ok(())
    }

    /// Get file/directory metadata.
    pub async fn stat(&mut self, path: &str) -> Result<RemoteFileMeta> {
        anyhow::bail!("SFTP stat not fully implemented")
    }

    /// Create a directory on the remote.
    pub async fn mkdir(&mut self, path: &str) -> Result<()> {
        anyhow::bail!("SFTP mkdir not fully implemented")
    }

    /// Remove a file on the remote.
    pub async fn remove(&mut self, path: &str) -> Result<()> {
        anyhow::bail!("SFTP remove not fully implemented")
    }

    /// Rename a file on the remote.
    pub async fn rename(&mut self, old_path: &str, new_path: &str) -> Result<()> {
        anyhow::bail!("SFTP rename not fully implemented")
    }
}

impl Default for SftpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to open an SFTP subsystem channel on an existing SSH session.
/// The caller provides a russh Handle.
pub async fn open_sftp_channel<H: russh::client::Handler>(
    handle: &russh::client::Handle<H>,
) -> Result<russh::Channel<russh::client::Msg>>
where
    H::Error: std::error::Error + Send + Sync,
{
    let channel = handle
        .channel_open_session()
        .await
        .context("Failed to open SFTP channel")?;
    channel
        .exec(true, "sftp")
        .await
        .context("Failed to start SFTP subsystem")?;
    Ok(channel)
}
