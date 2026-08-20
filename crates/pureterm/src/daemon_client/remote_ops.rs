//! 远程文件操作:listdir、read、write、find。

use std::io;
use std::path::Path;

use super::rpc;

/// 来自 `remote.listdir` 的一个目录条目(对应守护进程 `FileEntry`)。
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RemoteFileEntry {
    pub name: String,
    pub is_dir: bool,
    #[serde(default)]
    pub size: Option<u64>,
}

/// 来自 `remote.read` 的文本负载。
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RemoteFileRead {
    pub path: String,
    pub content: String,
    pub truncated: bool,
    #[serde(default)]
    pub size: Option<u64>,
}

/// 来自 `remote.find` 的匹配项。
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RemoteFindMatch {
    pub path: String,
    pub is_dir: bool,
}

/// GAP-43:列出守护进程会话上的目录(本地 `read_dir` 或经
/// 会话 ControlMaster socket 的远程 `ssh -S`)。
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

/// GAP-43:从守护进程会话(本地或 SSH)读取文件,上限为 `max_bytes` 字节。
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

/// GAP-43:在守护进程会话上写入文本文件。
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

/// GAP-43:在守护进程会话上按名称查找路径。
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