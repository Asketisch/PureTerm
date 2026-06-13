//! CLI client for PureTerm Socket API.
//!
//! Connects to a running PureTerm instance's Unix socket, sends a JSON-RPC
//! request, and returns the parsed response.

use std::io::{BufRead, BufReader, Write};

use super::listener::find_socket;
use super::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Send a JSON-RPC request to a running PureTerm instance.
///
/// Returns the parsed `result` value on success (or `None` if the response
/// carried no result field), or an `Err` message on failure. The caller owns
/// presentation (compact vs. pretty via `--json`) and exit handling — GAP-6.2
/// moved both out of the socket bridge so the CLI command groups can format
/// output uniformly.
pub fn send_rpc(
    method: &str,
    params: Option<serde_json::Value>,
    socket_override: Option<std::path::PathBuf>,
) -> Result<Option<serde_json::Value>, String> {
    let stream = find_socket(socket_override).map_err(|e| format!("Cannot connect to PureTerm: {e}"))?;

    // Build request with a simple sequential ID.
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let request = if let Some(p) = params {
        JsonRpcRequest::with_params(id, method, p)
    } else {
        JsonRpcRequest::new(id, method)
    };

    // Serialize and send.
    let mut json = serde_json::to_string(&request).map_err(|e| format!("Serialize error: {e}"))?;
    json.push('\n');

    let mut stream = stream;
    stream.write_all(json.as_bytes()).map_err(|e| format!("Write error: {e}"))?;
    stream.shutdown(std::net::Shutdown::Write).map_err(|e| format!("Shutdown error: {e}"))?;

    // Read response.
    let mut buffer = String::new();
    let mut reader = BufReader::new(&stream);
    reader.read_line(&mut buffer).map_err(|e| format!("Read error: {e}"))?;

    if buffer.trim().is_empty() {
        return Err("No response from PureTerm".into());
    }

    let resp: JsonRpcResponse =
        serde_json::from_str(&buffer).map_err(|e| format!("Invalid response: {e}\nRaw: {buffer}"))?;

    if let Some(err) = &resp.error {
        return Err(format!("Error ({}): {}", err.code, err.message));
    }

    Ok(resp.result)
}
