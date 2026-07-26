//! Remote file operations: listdir, read, write, find.

use std::io;
use std::path::Path;

use super::rpc;

/// One directory entry from `remote.listdir` (mirrors daemon `FileEntry`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RemoteFileEntry {
    pub name: String,
    pub is_dir: bool,
    #[serde(default)]
    pub size: Option<u64>,
}

/// Text payload from `remote.read`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RemoteFileRead {
    pub path: String,
    pub content: String,
    pub truncated: bool,
    #[serde(default)]
    pub size: Option<u64>,
}

/// Match from `remote.find`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RemoteFindMatch {
    pub path: String,
    pub is_dir: bool,
}

/// GAP-43: list a directory on a daemon session (local `read_dir` or remote
/// `ssh -S` over the session ControlMaster socket).
pub fn remote_listdir(
    socket: &Path,
    session_id: &str,
    path: &str,
) -> io::Result<Vec<RemoteFileEntry>> {
    let resp = rpc(
        socket,
        "remote.listdir",
        Some(serde_json::json!({ "session_id": session_id, "path": path })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let result = resp
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no result"))?;
    let entries = result
        .get("entries")
        .and_then(|v| v.as_array())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing entries array"))?;
    serde_json::from_value(serde_json::Value::Array(entries.clone()))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// GAP-43: read a file from a daemon session (local or SSH), capped at `max_bytes`.
pub fn remote_read(
    socket: &Path,
    session_id: &str,
    path: &str,
    max_bytes: u64,
) -> io::Result<RemoteFileRead> {
    let resp = rpc(
        socket,
        "remote.read",
        Some(serde_json::json!({
            "session_id": session_id,
            "path": path,
            "max_bytes": max_bytes,
        })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let result = resp
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no result"))?;
    serde_json::from_value(result).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// GAP-43: write a text file on a daemon session.
pub fn remote_write(
    socket: &Path,
    session_id: &str,
    path: &str,
    content: &str,
) -> io::Result<()> {
    let resp = rpc(
        socket,
        "remote.write",
        Some(serde_json::json!({
            "session_id": session_id,
            "path": path,
            "content": content,
        })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    Ok(())
}

/// GAP-43: find paths by name on a daemon session.
pub fn remote_find(
    socket: &Path,
    session_id: &str,
    base: &str,
    query: &str,
) -> io::Result<Vec<RemoteFindMatch>> {
    let resp = rpc(
        socket,
        "remote.find",
        Some(serde_json::json!({
            "session_id": session_id,
            "path": base,
            "query": query,
        })),
    )?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    let result = resp
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no result"))?;
    let matches = result
        .get("matches")
        .and_then(|v| v.as_array())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing matches array"))?;
    serde_json::from_value(serde_json::Value::Array(matches.clone()))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}