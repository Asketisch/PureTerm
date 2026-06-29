//! GAP-04 MVP: desktop screenshot (macOS `screencapture`) and input-injection stubs.

use std::path::PathBuf;
use std::process::Command;

/// Raw PNG bytes from a full-screen capture.
pub fn capture_screen_png() -> Result<Vec<u8>, String> {
    #[cfg(target_os = "macos")]
    {
        let tmp = temp_png_path();
        let path_str = tmp.to_str().unwrap_or("/tmp/pureterm-screenshot.png");
        let status = Command::new("screencapture")
            .args(["-x", "-t", "png", path_str])
            .status()
            .map_err(|e| format!("screencapture spawn failed: {e}"))?;
        if !status.success() {
            return Err(format!("screencapture exited with {status}"));
        }
        let bytes = std::fs::read(&tmp).map_err(|e| format!("read screenshot: {e}"))?;
        let _ = std::fs::remove_file(&tmp);
        if bytes.is_empty() {
            return Err("screenshot empty".into());
        }
        Ok(bytes)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("desktop screenshot: macOS-only in MVP (GAP-04)".into())
    }
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

fn temp_png_path() -> PathBuf {
    std::env::temp_dir().join(format!("pureterm-screenshot-{}.png", std::process::id()))
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
}
