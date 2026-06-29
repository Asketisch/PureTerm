//! Test fixture: a minimal LSP server over stdio.
//!
//! Used by `client.rs` regression tests for the initialize handshake (BUG-2),
//! graceful shutdown (BUG-4) and crash/reader-exit lifecycle (BUG-3). It is a
//! separate binary so tests can spawn it via `CARGO_BIN_EXE_mock_lsp_server`.
//!
//! Args:
//! - argv[1] = scenario
//! - argv[2] = marker path (optional); written when a `shutdown` request arrives.
//!
//! Scenarios:
//! - _unset_/"ok"             : full `initialize`→`initialized`→`shutdown`→`exit` dance.
//! - "fail-init"              : reply to `initialize` with a JSON-RPC error.
//! - "publish"                : like "ok" but emit one diagnostic after `didOpen`.
//! - "crash-after-publish"    : emit one diagnostic after `didOpen`, then close stdout
//!                              (simulates a server crash → reader EOF).

use std::fs;
use std::io::{BufRead, Write};

use serde_json::{json, Value};

fn main() {
    let scenario = std::env::args().nth(1).unwrap_or_default();
    let marker = std::env::args().nth(2);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut out = stdout.lock();

    while let Some(msg) = read_message(&mut reader) {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        match method {
            "initialize" => {
                let resp = if scenario == "fail-init" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32603, "message": "init refused" }
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "capabilities": { "hoverProvider": true } }
                    })
                };
                let _ = write_msg(&mut out, &resp);
            }
            "shutdown" => {
                // Write the marker BEFORE replying, so a client that unblocks on
                // the response and then SIGKILLs us still observes the marker.
                if let Some(p) = &marker {
                    let _ = fs::write(p, "shutdown\n");
                }
                if let Some(id) = id {
                    let _ = write_msg(&mut out, &json!({ "jsonrpc": "2.0", "id": id, "result": null }));
                }
            }
            "exit" => break,
            "textDocument/didOpen" if scenario == "publish" || scenario == "crash-after-publish" => {
                let _ = write_msg(&mut out, &json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": {
                        "uri": "file:///nonexistent/mock.rs",
                        "diagnostics": [{
                            "message": "mock diag",
                            "severity": 1,
                            "range": {
                                "start": { "line": 0, "character": 0 },
                                "end": { "line": 0, "character": 1 }
                            }
                        }]
                    }
                }));
                let _ = out.flush();
                if scenario == "crash-after-publish" {
                    // Simulate a crash: stop reading and close stdout.
                    return;
                }
            }
            _ => {}
        }
    }
}

/// Read one Content-Length-framed JSON message. `None` on EOF.
fn read_message<R: BufRead>(r: &mut R) -> Option<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line).ok()?;
        if n == 0 {
            return None;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
    }
    let len = content_length?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn write_msg<W: Write>(w: &mut W, v: &Value) -> std::io::Result<()> {
    let body = serde_json::to_string(v)?;
    write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    w.flush()
}
