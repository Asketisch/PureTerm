//! SSH SFTP file operations over an existing SSH session.
//!
//! Uses the russh SFTP subsystem to provide basic file operations.
//! Implements SFTP v3 protocol.

use anyhow::{Context, Result};
use russh::ChannelMsg;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// SFTP protocol version we implement.
const SFTP_VERSION: u32 = 3;

/// SFTP packet types (v3).
const SSH_FXP_INIT: u8 = 1;
const SSH_FXP_VERSION: u8 = 2;
const SSH_FXP_OPEN: u8 = 3;
const SSH_FXP_CLOSE: u8 = 4;
const SSH_FXP_READ: u8 = 5;
const SSH_FXP_WRITE: u8 = 6;
const SSH_FXP_READDIR: u8 = 11;
const SSH_FXP_STAT: u8 = 7;
const SSH_FXP_OPENDIR: u8 = 10;
const SSH_FXP_MKDIR: u8 = 14;
const SSH_FXP_REMOVE: u8 = 13;
const SSH_FXP_RENAME: u8 = 11;
const SSH_FXP_HANDLE: u8 = 101;
const SSH_FXP_DATA: u8 = 102;
const SSH_FXP_STATUS: u8 = 103;
const SSH_FXP_NAME: u8 = 104;
const SSH_FXP_ATTRS: u8 = 105;

/// SFTP status codes.
const SSH_FX_OK: u32 = 0;
const SSH_FX_EOF: u32 = 1;
const SSH_FX_NO_SUCH_FILE: u32 = 2;
const SSH_FX_PERMISSION_DENIED: u32 = 3;

/// File open flags.
const SSH_FXF_READ: u32 = 0x1;
const SSH_FXF_WRITE: u32 = 0x2;
const SSH_FXF_APPEND: u32 = 0x4;
const SSH_FXF_CREAT: u32 = 0x8;
const SSH_FXF_TRUNC: u32 = 0x10;
const SSH_FXF_EXCL: u32 = 0x20;

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
    channel: Option<russh::Channel<russh::client::Msg>>,
    next_id: u32,
}

/// SFTP response.
#[derive(Debug)]
pub enum SftpResponse {
    Handle { handle: Vec<u8> },
    Names { entries: Vec<RemoteFileMeta> },
    Data { data: Vec<u8> },
    Status { code: u32, message: String },
    Meta { meta: RemoteFileMeta },
}

// ============================================================================
// Helper functions (standalone, not methods)
// ============================================================================

/// Send a packet on the SFTP channel.
async fn sftp_send_packet(channel: &mut russh::Channel<russh::client::Msg>, data: &[u8]) -> Result<()> {
    let len = (data.len() as u32).to_be_bytes();
    let mut packet = len.to_vec();
    packet.extend_from_slice(data);
    channel.data(&packet[..]).await?;
    Ok(())
}

/// Read a packet from the SFTP channel.
async fn sftp_read_packet(channel: &mut russh::Channel<russh::client::Msg>) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];

    // Read length prefix
    let mut remaining = 4;
    while remaining > 0 {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                let d = data.as_ref();
                let n = d.len().min(remaining);
                len_buf[4 - remaining..][..n].copy_from_slice(&d[..n]);
                remaining -= n;
            }
            Some(ChannelMsg::Eof) | None => {
                return Ok(Vec::new());
            }
            _ => {}
        }
    }

    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    let mut remaining = len;
    while remaining > 0 {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                let d = data.as_ref();
                let n = d.len().min(remaining);
                buf[len - remaining..][..n].copy_from_slice(&d[..n]);
                remaining -= n;
            }
            Some(ChannelMsg::Eof) | None => {
                return Ok(Vec::new());
            }
            _ => {}
        }
    }
    Ok(buf)
}

/// Encode a filename attribute block (simplified).
fn encode_filename(name: &str, longname: &str, _is_dir: bool, size: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0u8; 4]); // flags placeholder
    // Encode filename
    buf.extend_from_slice(&(name.len() as u32).to_be_bytes());
    buf.extend_from_slice(name.as_bytes());
    // Encode long name (ls -l style)
    buf.extend_from_slice(&(longname.len() as u32).to_be_bytes());
    buf.extend_from_slice(longname.as_bytes());
    // Encode attrs: size + is_dir
    buf.extend_from_slice(&[0u8]); // ATTR_SIZE flag only
    buf.extend_from_slice(&size.to_be_bytes());
    buf
}

/// Parse names data.
fn parse_names(data: &[u8], count: usize) -> Vec<RemoteFileMeta> {
    let mut entries = Vec::new();
    let mut pos = 0;

    for _ in 0..count {
        if pos + 8 > data.len() {
            break;
        }

        let name_len = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        pos += 4;
        if pos + name_len > data.len() {
            break;
        }
        let name = std::str::from_utf8(&data[pos..pos+name_len]).unwrap_or("?").to_string();
        pos += name_len;

        let longname_len = if pos + 4 <= data.len() {
            u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize
        } else {
            0
        };
        pos += 4;
        let longname = if pos + longname_len <= data.len() {
            std::str::from_utf8(&data[pos..pos+longname_len]).unwrap_or("")
        } else {
            ""
        };
        pos += longname_len;

        // Skip attrs for now
        if pos + 4 > data.len() {
            break;
        }
        let flags = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
        pos += 4;

        let mut size = 0u64;
        if flags & 0x1 != 0 {
            if pos + 8 > data.len() {
                break;
            }
            size = u64::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3], data[pos+4], data[pos+5], data[pos+6], data[pos+7]]);
            pos += 8;
        }

        let is_dir = longname.starts_with('d') || name == "." || name == "..";
        entries.push(RemoteFileMeta {
            name: name.clone(),
            path: name,
            is_dir,
            size,
            modified: None,
            permissions: None,
        });
    }

    entries
}

// ============================================================================
// SftpClient impl
// ============================================================================

impl SftpClient {
    /// Create a new SFTP client. Opens SFTP subsystem on the given handle.
    pub async fn new(handle: &russh::client::Handle<impl russh::client::Handler>) -> Result<Self> {
        let mut client = Self {
            channel: None,
            next_id: 1,
        };
        client.init(handle).await?;
        Ok(client)
    }

    /// Initialize SFTP protocol version exchange.
    async fn init<H: russh::client::Handler>(&mut self, handle: &russh::client::Handle<H>) -> Result<()> {
        // Open session channel
        let mut channel = handle
            .channel_open_session()
            .await
            .context("Failed to open SFTP channel")?;

        // Request SFTP subsystem
        channel
            .exec(true, "sftp")
            .await
            .context("Failed to start SFTP subsystem")?;

        // Send INIT packet with version
        let mut buf = Vec::new();
        buf.extend_from_slice(&[SSH_FXP_INIT]);
        buf.extend_from_slice(&SFTP_VERSION.to_be_bytes());
        sftp_send_packet(&mut channel, &buf).await?;

        // Read VERSION response
        let resp = sftp_read_packet(&mut channel).await?;
        if resp.is_empty() {
            anyhow::bail!("SFTP: connection closed during init");
        }
        let typ = resp[4];
        if typ != SSH_FXP_VERSION {
            anyhow::bail!("SFTP: expected VERSION response, got type {}", typ);
        }

        self.channel = Some(channel);
        Ok(())
    }

    /// Get the next request ID.
    fn next_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Parse an SFTP names response.
    fn parse_names_response(&self, data: &[u8]) -> Result<Vec<RemoteFileMeta>> {
        let mut pos = 0;
        let count = if data.len() >= 4 {
            u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize
        } else {
            0
        };
        pos += 4;

        let mut entries = Vec::new();
        for _ in 0..count {
            if pos + 8 > data.len() {
                break;
            }

            // Skip filename (we use longname)
            let name_len = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            pos += 4;
            if pos + name_len > data.len() {
                break;
            }
            let name = std::str::from_utf8(&data[pos..pos+name_len])
                .unwrap_or("?").to_string();
            pos += name_len;

            // Read longname
            let longname_len = if pos + 4 <= data.len() {
                u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize
            } else {
                0
            };
            pos += 4;
            let longname = if pos + longname_len <= data.len() {
                std::str::from_utf8(&data[pos..pos+longname_len]).unwrap_or("")
            } else {
                ""
            };
            pos += longname_len;

            // Read attrs
            if pos + 5 > data.len() {
                break;
            }
            pos += 1; // flags
            let size = u64::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3], data[pos+4], data[pos+5], data[pos+6], data[pos+7]]);
            pos += 8;

            let is_dir = longname.starts_with('d') || longname.contains(" <dir>");
            entries.push(RemoteFileMeta {
                name: name.clone(),
                path: name,
                is_dir,
                size,
                modified: None,
                permissions: None,
            });
        }

        Ok(entries)
    }

    /// List files in a remote directory.
    pub async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteFileMeta>> {
        let id = self.next_id();
        let channel = self.channel.as_mut().context("SFTP not initialized")?;

        // Send OPENDIR request
        let mut req = Vec::new();
        req.extend_from_slice(&id.to_be_bytes());
        req.extend_from_slice(&(path.len() as u32).to_be_bytes());
        req.extend_from_slice(path.as_bytes());

        let mut packet = vec![SSH_FXP_OPENDIR];
        packet.extend_from_slice(&req);
        sftp_send_packet(channel, &packet).await?;

        // Read response
        let resp = sftp_read_packet(channel).await?;
        if resp.len() < 5 {
            anyhow::bail!("SFTP: short OPENDIR response");
        }
        let resp_id = u32::from_be_bytes([resp[0], resp[1], resp[2], resp[3]]);
        let typ = resp[4];
        if resp_id != id {
            anyhow::bail!("SFTP: ID mismatch in OPENDIR response");
        }

        if typ == SSH_FXP_STATUS {
            let code = u32::from_be_bytes([resp[5], resp[6], resp[7], resp[8]]);
            anyhow::bail!("SFTP: OPENDIR failed with status {}", code);
        }

        if typ != SSH_FXP_HANDLE {
            anyhow::bail!("SFTP: expected HANDLE response, got type {}", typ);
        }

        // Extract handle
        let handle_len = u32::from_be_bytes([resp[5], resp[6], resp[7], resp[8]]) as usize;
        let handle = resp[9..9+handle_len].to_vec();

        // Read directory entries
        let mut entries = Vec::new();
        loop {
            let id = {
                // Scope to release borrow of self before async call
                let id = self.next_id();
                let mut req = Vec::new();
                req.extend_from_slice(&id.to_be_bytes());
                req.extend_from_slice(&handle_len.to_be_bytes());
                req.extend_from_slice(&handle);

                let mut packet = vec![SSH_FXP_READDIR];
                packet.extend_from_slice(&req);
                let channel = self.channel.as_mut().context("SFTP not initialized")?;
                sftp_send_packet(channel, &packet).await?;

                let resp = sftp_read_packet(channel).await?;
                if resp.len() < 5 {
                    break;
                }

                let _resp_id = u32::from_be_bytes([resp[0], resp[1], resp[2], resp[3]]);
                let typ = resp[4];

                if typ == SSH_FXP_STATUS {
                    let code = u32::from_be_bytes([resp[5], resp[6], resp[7], resp[8]]);
                    if code == SSH_FX_EOF {
                        break;
                    }
                }

                if typ == SSH_FXP_NAME {
                    let count = u32::from_be_bytes([resp[5], resp[6], resp[7], resp[8]]) as usize;
                    // Parse names - simplified parsing
                    let parsed = parse_names(&resp[9..], count);
                    entries.extend(parsed);
                }
                id
            };
            // id goes out of scope here, releasing borrow of self
        }

        // Close handle
        let id = self.next_id();
        let mut req = Vec::new();
        req.extend_from_slice(&id.to_be_bytes());
        req.extend_from_slice(&handle_len.to_be_bytes());
        req.extend_from_slice(&handle);

        let mut packet = vec![SSH_FXP_CLOSE];
        packet.extend_from_slice(&req);
        let channel = self.channel.as_mut().context("SFTP not initialized")?;
        sftp_send_packet(channel, &packet).await?;

        Ok(entries)
    }

    /// Get file/directory metadata.
    pub async fn stat(&mut self, path: &str) -> Result<RemoteFileMeta> {
        let id = self.next_id();
        let channel = self.channel.as_mut().context("SFTP not initialized")?;

        let mut req = Vec::new();
        req.extend_from_slice(&id.to_be_bytes());
        req.extend_from_slice(&(path.len() as u32).to_be_bytes());
        req.extend_from_slice(path.as_bytes());

        let mut packet = vec![SSH_FXP_STAT];
        packet.extend_from_slice(&req);
        sftp_send_packet(channel, &packet).await?;

        let resp = sftp_read_packet(channel).await?;
        if resp.len() < 5 {
            anyhow::bail!("SFTP: short STAT response");
        }

        let typ = resp[4];
        if typ == SSH_FXP_STATUS {
            let code = u32::from_be_bytes([resp[5], resp[6], resp[7], resp[8]]);
            anyhow::bail!("SFTP: stat failed with status {}", code);
        }

        // Parse attrs - simplified
        let is_dir = path.ends_with('/') || path.is_empty();
        Ok(RemoteFileMeta {
            name: Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path)
                .to_string(),
            path: path.to_string(),
            is_dir,
            size: 0,
            modified: None,
            permissions: None,
        })
    }

    /// Create a directory on the remote.
    pub async fn mkdir(&mut self, path: &str) -> Result<()> {
        let id = self.next_id();
        let channel = self.channel.as_mut().context("SFTP not initialized")?;

        let mut req = Vec::new();
        req.extend_from_slice(&id.to_be_bytes());
        req.extend_from_slice(&(path.len() as u32).to_be_bytes());
        req.extend_from_slice(path.as_bytes());
        req.extend_from_slice(&0u32.to_be_bytes()); // no attrs

        let mut packet = vec![SSH_FXP_MKDIR];
        packet.extend_from_slice(&req);
        sftp_send_packet(channel, &packet).await?;

        let resp = sftp_read_packet(channel).await?;
        if resp.len() < 5 {
            anyhow::bail!("SFTP: short MKDIR response");
        }

        let typ = resp[4];
        if typ == SSH_FXP_STATUS {
            let code = u32::from_be_bytes([resp[5], resp[6], resp[7], resp[8]]);
            if code != SSH_FX_OK {
                anyhow::bail!("SFTP: mkdir failed with status {}", code);
            }
        }
        Ok(())
    }

    /// Remove a file on the remote.
    pub async fn remove(&mut self, path: &str) -> Result<()> {
        let id = self.next_id();
        let channel = self.channel.as_mut().context("SFTP not initialized")?;

        let mut req = Vec::new();
        req.extend_from_slice(&id.to_be_bytes());
        req.extend_from_slice(&(path.len() as u32).to_be_bytes());
        req.extend_from_slice(path.as_bytes());

        let mut packet = vec![SSH_FXP_REMOVE];
        packet.extend_from_slice(&req);
        sftp_send_packet(channel, &packet).await?;

        let resp = sftp_read_packet(channel).await?;
        if resp.len() < 5 {
            anyhow::bail!("SFTP: short REMOVE response");
        }

        let typ = resp[4];
        if typ == SSH_FXP_STATUS {
            let code = u32::from_be_bytes([resp[5], resp[6], resp[7], resp[8]]);
            if code != SSH_FX_OK {
                anyhow::bail!("SFTP: remove failed with status {}", code);
            }
        }
        Ok(())
    }

    /// Rename a file on the remote.
    pub async fn rename(&mut self, old_path: &str, new_path: &str) -> Result<()> {
        let id = self.next_id();
        let channel = self.channel.as_mut().context("SFTP not initialized")?;

        let mut req = Vec::new();
        req.extend_from_slice(&id.to_be_bytes());
        req.extend_from_slice(&(old_path.len() as u32).to_be_bytes());
        req.extend_from_slice(old_path.as_bytes());
        req.extend_from_slice(&(new_path.len() as u32).to_be_bytes());
        req.extend_from_slice(new_path.as_bytes());
        req.extend_from_slice(&0u32.to_be_bytes()); // no flags

        let mut packet = vec![SSH_FXP_RENAME];
        packet.extend_from_slice(&req);
        sftp_send_packet(channel, &packet).await?;

        let resp = sftp_read_packet(channel).await?;
        if resp.len() < 5 {
            anyhow::bail!("SFTP: short RENAME response");
        }

        let typ = resp[4];
        if typ == SSH_FXP_STATUS {
            let code = u32::from_be_bytes([resp[5], resp[6], resp[7], resp[8]]);
            if code != SSH_FX_OK {
                anyhow::bail!("SFTP: rename failed with status {}", code);
            }
        }
        Ok(())
    }

    /// Close the SFTP session.
    pub async fn close(&mut self) -> Result<()> {
        if let Some(mut channel) = self.channel.take() {
            let id = self.next_id();
            let mut packet = vec![SSH_FXP_CLOSE];
            // Send empty handle to close (no handle to close since we don't keep one)
            packet.extend_from_slice(&id.to_be_bytes());
            packet.extend_from_slice(&0u32.to_be_bytes()); // empty handle
            let len = (packet.len() as u32).to_be_bytes();
            let mut full_packet = len.to_vec();
            full_packet.extend_from_slice(&packet);
            let _ = channel.data(&full_packet[..]).await;
        }
        Ok(())
    }
}

impl Drop for SftpClient {
    fn drop(&mut self) {
        // Best-effort close
        let channel = self.channel.take();
        if let Some(ch) = channel {
            tokio::spawn(async move {
                let id = 1u32;
                let mut packet = vec![SSH_FXP_CLOSE];
                packet.extend_from_slice(&id.to_be_bytes());
                packet.extend_from_slice(&0u32.to_be_bytes());
                let _ = ch.data(&packet[..]).await;
            });
        }
    }
}

/// Open an SFTP client on an existing SSH session.
pub async fn open_sftp<H: russh::client::Handler>(
    handle: &russh::client::Handle<H>,
) -> Result<SftpClient>
where
    H::Error: std::error::Error + Send + Sync,
{
    SftpClient::new(handle).await
}
