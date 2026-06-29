//! Regression tests for GAP-35 LSP client lifecycle bugs.
//!
//! These drive a mock LSP server (`src/bin/mock-lsp-server.rs`) over real stdio,
//! covering the initialize handshake (BUG-2), graceful shutdown (BUG-4) and
//! crash/reader-exit lifecycle (BUG-3). Lives in `tests/` so Cargo injects
//! `CARGO_BIN_EXE_mock_lsp_server`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pureterm_lsp::{LspClient, LspClientConfig};

/// Locate the `mock-lsp-server` binary built next to this test binary.
///
/// Cargo places the test exe at `<target>/<profile>/deps/<name>-<hash>` and the
/// package's bins at `<target>/<profile>/<bin>`, so walking two parents up from
/// the test exe lands on the profile dir. (Done manually because this workspace
/// does not inject `CARGO_BIN_EXE_*`.)
fn mock_exe() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("locate target profile dir");
    profile_dir.join("mock-lsp-server")
}

fn mock_cfg(scenario: &str, marker: Option<&str>) -> LspClientConfig {
    let mut args = vec![scenario.to_string()];
    if let Some(m) = marker {
        args.push(m.to_string());
    }
    LspClientConfig {
        command: mock_exe().to_string_lossy().into_owned(),
        args,
        workspace_root: PathBuf::from("."),
    }
}

/// BUG-2: `initialize` must read the server's response and surface a rejection
/// instead of fire-and-forgetting it.
#[test]
fn spawn_fails_when_server_rejects_initialize() {
    let result = LspClient::spawn(mock_cfg("fail-init", None));
    match result {
        Ok(_) => panic!("spawn must surface a server-side initialize error, but succeeded"),
        Err(_e) => {}
    }
}

/// BUG-4: `Drop` must perform the graceful shutdown/exit handshake rather than
/// SIGKILLing the server.
#[test]
fn drop_sends_graceful_shutdown() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("shutdown.marker");

    {
        let _client = LspClient::spawn(mock_cfg("ok", marker.to_str())).expect("spawn ok");
    } // dropped here

    let mut content = String::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        content = std::fs::read_to_string(&marker).unwrap_or_default();
        if content.contains("shutdown") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        content.contains("shutdown"),
        "Drop must send a `shutdown` request the server observes; marker was {content:?}"
    );
}

/// BUG-3: after the server crashes, diagnostics must be cleared and the client
/// marked dead (no stale diagnostics served to status bar / AI chat).
#[test]
fn clears_diagnostics_and_marks_dead_after_server_crash() {
    let client = LspClient::spawn(mock_cfg("crash-after-publish", None)).expect("spawn ok");

    // didOpen makes the mock publish one diagnostic, then close stdout.
    let file = tempfile::Builder::new()
        .suffix(".rs")
        .tempfile()
        .expect("tempfile");
    std::fs::write(file.path(), "fn main() {}").expect("write");
    let _ = client.did_open(file.path());

    // Wait for the reader to hit EOF and react.
    let deadline = Instant::now() + Duration::from_secs(3);
    while client.is_alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!client.is_alive(), "client must be marked dead after crash");

    let summary = client.summary();
    assert_eq!(
        summary.errors, 0,
        "stale diagnostics must be cleared after the server died"
    );
}
