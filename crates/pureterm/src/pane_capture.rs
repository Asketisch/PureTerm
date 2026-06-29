//! GAP-60: Pane screenshot / export — text snapshot to file MVP.

use std::path::Path;

/// `[pane_capture]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneCaptureConfig {
    pub enabled: bool,
    pub default_format: String,
}

impl Default for PaneCaptureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_format: "txt".into(),
        }
    }
}

/// Export terminal text content to a file. PNG rendering deferred; `.txt` always works.
pub fn export_text_snapshot(content: &str, output: &Path) -> Result<(), String> {
    if content.is_empty() {
        return Err("empty snapshot".into());
    }
    std::fs::write(output, content).map_err(|e| e.to_string())
}

/// Export with format hint (`txt` | `png`). PNG writes a placeholder note until GPU capture lands.
pub fn export_snapshot(content: &str, output: &Path, format: &str) -> Result<(), String> {
    match format.to_ascii_lowercase().as_str() {
        "txt" | "text" => export_text_snapshot(content, output),
        "png" => {
            let note = format!(
                "# PureTerm pane capture (PNG deferred)\n# Lines: {}\n\n{content}",
                content.lines().count()
            );
            export_text_snapshot(&note, output)
        }
        other => Err(format!("unsupported format: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn export_txt_roundtrip() {
        let dir = std::env::temp_dir().join(format!("pt-cap-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.txt");
        export_text_snapshot("hello\nworld", &path).unwrap();
        let s = std::fs::read_to_string(&path).unwrap();
        assert!(s.contains("hello"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
