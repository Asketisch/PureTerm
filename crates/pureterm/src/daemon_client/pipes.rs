//! Pipe-pane operations: set, list.

use std::io;
use std::path::Path;

use super::rpc;

/// GAP-6.6: set a session's pipe-pane tap (`command` None ⇒ clear). The GUI
/// resolves a pane to its daemon `session_id` and calls this.
pub fn pipe_set(socket: &Path, session_id: &str, command: Option<&str>) -> io::Result<()> {
    let mut params = serde_json::json!({ "session_id": session_id });
    if let Some(c) = command {
        params["command"] = serde_json::Value::String(c.to_string());
    }
    let resp = rpc(socket, "pipe.set", Some(params))?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    Ok(())
}

/// GAP-6.6: daemon session ids that currently have an active pipe-pane tap.
pub fn pipe_list(socket: &Path) -> io::Result<Vec<String>> {
    let resp = rpc(socket, "pipe.list", None)?;
    if let Some(err) = &resp.error {
        return Err(io::Error::new(io::ErrorKind::Other, err.message.clone()));
    }
    Ok(resp
        .result
        .and_then(|r| r.get("sessions").cloned())
        .and_then(|s| s.as_array().map(|a| a.clone()))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect())
}