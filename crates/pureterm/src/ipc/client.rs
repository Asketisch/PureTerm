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
    // Printed as a raw string (no format args) so JSON example braces
    // are not interpreted as format placeholders.
    let help = "\
Usage: pureterm msg <method> [JSON_PARAMS]

Send a JSON-RPC request to a running PureTerm instance.
GAP-6.1: 30+ methods across 8 namespaces.

system.*
  system.ping                      Health check
  system.version                   Show version
  system.info                      OS / version / tab & pane counts
  system.methods                   List all available methods

window.*
  window.list                      List open windows
  window.create                    Create new tab (single-window mode)
  window.close                     Close active tab
  window.focus                     Focus window

tab.*
  tab.list                         List tabs with id/title/panes
  tab.new                          Create a new tab
  tab.close [{\"index\":N}]           Close a tab by index
  tab.switch {\"index\":N}            Switch to a tab by index

pane.*
  pane.list                        List all panes
  pane.split {\"direction\":\"v|h\"}    Split active pane
  pane.close                       Close active pane
  pane.focus {\"direction\":\"next|prev\"}
  pane.swap                        Swap pane with sibling
  pane.zoom                        Toggle pane zoom

surface.*
  surface.list                     List surfaces (PTY instances)
  surface.send_text {\"text\":\"...\"}  Send text to a surface
  surface.resize                   (note: use pane.split)
  surface.snapshot                 (not yet implemented)

config.*
  config.get [{\"key\":\"...\"}]         Get config key(s)
  config.list                      List known config keys
  config.reload                    Reload config from disk
  config.set                       (use settings UI instead)

ssh.*
  ssh.list_profiles                List configured SSH profiles
  ssh.connect {\"index\":N}           Connect to a profile
  ssh.disconnect                   (close the SSH tab)

debug.*
  debug.ping                       Debug health check
  debug.dump_state                 Dump app state summary

Examples:
  pureterm msg system.ping
  pureterm msg system.methods
  pureterm msg tab.new
  pureterm msg tab.switch '{\"index\":1}'
  pureterm msg pane.split '{\"direction\":\"horizontal\"}'
  pureterm msg ssh.list_profiles
  pureterm msg ssh.connect '{\"index\":0}'
";
    eprint!("{help}");
}
