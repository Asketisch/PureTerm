//! CLI client for PureTerm Socket API.
//!
//! Connects to a running PureTerm instance's Unix socket, sends a JSON-RPC
//! request, and prints the response to stdout.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use super::listener::find_socket;
use super::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Send a JSON-RPC request to a running PureTerm instance and print the
/// response to stdout. Returns Ok(()) on success, Err with a message on
/// failure.
pub fn send_rpc(method: &str, params: Option<serde_json::Value>, socket_override: Option<std::path::PathBuf>) -> Result<(), String> {
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

    // Parse and pretty-print.
    let resp: JsonRpcResponse = serde_json::from_str(&buffer)
        .map_err(|e| format!("Invalid response: {e}\nRaw: {buffer}"))?;

    if let Some(err) = &resp.error {
        eprintln!("Error ({}): {}", err.code, err.message);
        std::process::exit(1);
    }

    if let Some(result) = &resp.result {
        let pretty = serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string());
        println!("{pretty}");
    }

    Ok(())
}

/// Print help text for the `pureterm msg` command.
pub fn print_help() {
    eprintln!("Usage: pureterm msg <method> [JSON_PARAMS]");
    eprintln!();
    eprintln!("Send a JSON-RPC request to a running PureTerm instance.");
    eprintln!();
    eprintln!("Built-in methods:");
    eprintln!("  system.ping            Health check");
    eprintln!("  system.version         Show version");
    eprintln!("  window.list            List open windows");
    eprintln!("  pane.list              List all panes");
    eprintln!("  surface.list           List all surfaces");
    eprintln!("  config.get             Get config values");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  pureterm msg system.ping");
    eprintln!("  pureterm msg system.version");
    eprintln!("  pureterm msg pane.list");
}
