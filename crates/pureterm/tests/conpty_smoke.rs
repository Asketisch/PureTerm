//! T-core-windows-conpty v3 E2E (checklist E.6).
//!
//! Windows-only integration test: spawns a real ConPTY-backed
//! `cmd.exe` + `powershell.exe` and confirms the child writes its
//! banner back through the conout pipe. This is the first test that
//! exercises the Windows-specific spawn path end-to-end; all other
//! tests in the workspace are Unix-friendly (or `#[cfg(unix)]`-gated).
//!
//! Run on a `windows-2022` runner with:
//!   cargo test -p pureterm --test conpty_smoke -- --nocapture
//!
//! On non-Windows hosts the entire file compiles to nothing — the
//! `#![cfg(windows)]` guard at the top short-circuits the module.
//!
//! ## Verification approach
//!
//! 1. Call `windows::open_conpty()` (the v3 real ConPTY impl in
//!    `crates/pureterm/src/windows.rs`). The function returns a
//!    `ConptyHandle` owning the `HPCON` + the two anonymous pipes.
//! 2. Spawn `cmd.exe /c echo CONPTY-SMOKE-MARKER` as the ConPTY child
//!    (via `CreateProcessW` with the pseudo-console attribute list).
//! 3. Read from the conout end until the marker appears (or a 2s
//!    timeout).
//! 4. Close the ConPTY, reaping the child.
//!
//! The test is intentionally minimal — full PTY semantics (resize,
//! alternate screen, mouse) are covered by alacritty_terminal's own
//! test suite; this just verifies the PureTerm-side wrapper links
//! the Win32 ConPTY API correctly.

#![cfg(windows)]

use std::process::Command;
use std::time::{Duration, Instant};

/// Drain conout until `marker` appears or `timeout` elapses.
fn wait_for_output_contains(
    out: &mut impl std::io::Read,
    marker: &str,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    let mut buf = String::new();
    let mut chunk = [0u8; 1024];
    while Instant::now() < deadline {
        match out.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
                if buf.contains(marker) {
                    return true;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
    false
}

#[test]
fn cmd_exe_smoke() {
    // The marker cmd echoes back. We expect ConPTY to forward stdout
    // to our conout pipe transparently.
    const MARKER: &str = "CONPTY-CMD-SMOKE";

    // v3 implementation note: `windows::open_conpty` exists in
    // `crates/pureterm/src/windows.rs`. In v2 it returns
    // `Err("ConPTY not wired on Windows in v2…")`; v3 swaps in a real
    // `CreatePseudoConsole` + `CreateProcessW` chain.
    //
    // Until the full v3 ConPTY impl lands, this test body is
    // intentionally `todo!()` so the file compiles on Windows but
    // panics on run — keeping the CI matrix honest about the gap.
    let _ = Command::new("cmd.exe")
        .args(["/c", &format!("echo {MARKER}")])
        .output();
    // Smoke: the cmd.exe binary itself is reachable.
    // TODO(t-core-windows-conpty v3+1): replace with real ConPTY
    // spawn once `windows::open_conpty` is wired.
    todo!("real ConPTY spawn pending windows::open_conpty v3+1");
}

#[test]
fn powershell_smoke() {
    const MARKER: &str = "CONPTY-PS-SMOKE";
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &format!("Write-Output '{MARKER}'")])
        .output();
    let _ = wait_for_output_contains;
    // TODO(t-core-windows-conpty v3+1): replace with real ConPTY
    // spawn once `windows::open_conpty` is wired.
    todo!("real ConPTY spawn pending windows::open_conpty v3+1");
}