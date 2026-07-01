//! GAP-04 MVP: desktop screenshot (macOS `screencapture`) and input-injection stubs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Raw PNG bytes from a full-screen capture.
pub fn capture_screen_png() -> Result<Vec<u8>, String> {
    #[cfg(target_os = "macos")]
    {
        let tmp = temp_png_path();
        // `screencapture` takes the destination as a CLI arg, so the path must
        // be UTF-8. A non-UTF-8 $TMPDIR is unsupported — fail rather than fall
        // back to a shared hardcoded path (the old `unwrap_or` dropped even the
        // PID, colliding across instances and offering a static symlink target).
        let path_str = tmp
            .to_str()
            .ok_or_else(|| "screenshot: temp dir path is not UTF-8 (GAP-04)".to_string())?;
        let status = Command::new("screencapture")
            .args(["-x", "-t", "png", path_str])
            .status()
            .map_err(|e| format!("screencapture spawn failed: {e}"))?;
        if !status.success() {
            // screencapture may have written a partial file — best-effort
            // cleanup, then surface the failure without reading bad bytes.
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("screencapture exited with {status}"));
        }
        read_and_cleanup(&tmp)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("desktop screenshot: macOS-only in MVP (GAP-04)".into())
    }
}

/// Read the captured PNG then **always** remove the temp file — on success,
/// on read failure, and when the file is missing. The capture contract is that
/// `screencapture` created the file and we own its cleanup on every path; the
/// previous version leaked it whenever `read` failed (the `?` skipped cleanup).
/// Returns `Err` for an empty capture (screencapture wrote nothing usable).
fn read_and_cleanup(path: &Path) -> Result<Vec<u8>, String> {
    let read = std::fs::read(path);
    // Unconditional best-effort cleanup on every path — never leak the temp PNG.
    let _ = std::fs::remove_file(path);
    let bytes = read.map_err(|e| format!("read screenshot: {e}"))?;
    if bytes.is_empty() {
        return Err("screenshot empty".into());
    }
    Ok(bytes)
}

/// Mouse click — MVP stub (returns structured not-implemented error).
pub fn click(x: i32, y: i32) -> Result<(), String> {
    let _ = (x, y);
    Err(format!(
        "desktop_click({x},{y}): input injection stub — not yet implemented (GAP-04 MVP)"
    ))
}

/// Type text into the focused application — MVP stub.
pub fn type_text(text: &str) -> Result<(), String> {
    let _ = text;
    Err("desktop_type: input injection stub — not yet implemented (GAP-04 MVP)".into())
}

/// Press a key by name — MVP stub.
pub fn key_press(key: &str) -> Result<(), String> {
    let _ = key;
    Err(format!(
        "desktop_key_press({key:?}): input injection stub — not yet implemented (GAP-04 MVP)"
    ))
}

/// A per-capture temp path that is unique and hard to predict, to mitigate the
/// classic `/tmp` symlink race (a PID-only name is guessable; the old static
/// fallback was worse). PID + high-resolution timestamp + in-process counter
/// keeps this std-only (no `tempfile` dep). Residual risk on a true multi-user
/// hardened host is recorded as an accepted MVP limitation.
fn temp_png_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("pureterm-screenshot-{pid}-{nanos}-{n}.png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_stubs_return_errors() {
        assert!(click(10, 20).is_err());
        assert!(type_text("hello").is_err());
        assert!(key_press("Return").is_err());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn screenshot_non_macos_errors() {
        assert!(capture_screen_png().is_err());
    }

    #[test]
    fn read_and_cleanup_removes_file_on_success() {
        let p = std::env::temp_dir().join("pureterm-test-cleanup-ok.png");
        std::fs::write(&p, b"fake-png-bytes").unwrap();
        let bytes = read_and_cleanup(&p).unwrap();
        assert_eq!(bytes, b"fake-png-bytes");
        assert!(!p.exists(), "temp file must be removed on success");
    }

    #[test]
    fn read_and_cleanup_removes_file_and_errors_when_empty() {
        let p = std::env::temp_dir().join("pureterm-test-cleanup-empty.png");
        std::fs::write(&p, b"").unwrap();
        let err = read_and_cleanup(&p).unwrap_err();
        assert!(err.contains("empty"), "unexpected err: {err}");
        assert!(!p.exists(), "temp file must be removed even when empty");
    }

    #[test]
    fn read_and_cleanup_errors_without_panic_when_missing() {
        let p = std::env::temp_dir().join("pureterm-test-cleanup-missing.png");
        let _ = std::fs::remove_file(&p); // ensure absent
        assert!(read_and_cleanup(&p).is_err());
    }

    #[test]
    fn temp_png_paths_are_distinct() {
        // Consecutive paths must differ — the PID+nanos+counter name is the
        // symlink-race mitigation; a colliding name would defeat it.
        let a = temp_png_path();
        let b = temp_png_path();
        assert_ne!(a, b);
    }
}
