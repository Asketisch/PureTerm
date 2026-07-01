//! GAP-34: File preview — image/PDF/text path detection + open helper.
//!
//! Detects the preview kind (image/PDF/text) from a file extension and opens it
//! via the platform handler. On macOS QuickLook (`qlmanage -p`) is preferred for
//! images/PDFs; text/unknown fall back to the default `open` handler.
//! Wired into the app via `Cmd+Y` (`Message::PreviewFile`).

use std::path::Path;

/// Detected preview kind for a filesystem path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    Text,
    Image,
    Pdf,
    Unknown,
}

/// `[file_preview]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePreviewConfig {
    pub enabled: bool,
}

impl Default for FilePreviewConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

pub fn detect_kind(path: &Path) -> PreviewKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => PreviewKind::Image,
        "pdf" => PreviewKind::Pdf,
        "txt" | "md" | "rs" | "toml" | "json" | "yaml" | "yml" | "log" | "ts" | "js" | "py"
        | "go" | "c" | "h" | "cpp" | "hpp" | "java" | "rb" | "sh" | "css" | "html" | "xml"
        | "sql" | "kt" | "swift" | "lua" | "vim" | "dart" => PreviewKind::Text,
        _ => PreviewKind::Unknown,
    }
}

/// Open a file preview via the platform handler.
///
/// macOS: QuickLook (`qlmanage -p`) for images/PDFs; `open` for text/unknown.
/// Linux: `xdg-open`. Other platforms: unsupported.
pub fn open_preview(path: &Path, cfg: &FilePreviewConfig) -> Result<(), String> {
    if !cfg.enabled {
        return Err("file_preview disabled".into());
    }
    if !path.exists() {
        return Err(format!("path not found: {}", path.display()));
    }
    let kind = detect_kind(path);
    #[cfg(target_os = "macos")]
    {
        if matches!(kind, PreviewKind::Image | PreviewKind::Pdf) {
            std::process::Command::new("qlmanage")
                .args(["-p"])
                .arg(path)
                .status()
                .map_err(|e| e.to_string())?;
        } else {
            std::process::Command::new("open")
                .arg(path)
                .status()
                .map_err(|e| e.to_string())?;
        }
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .status()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = kind;
        return Err("file preview not supported on this platform".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_extensions() {
        assert_eq!(detect_kind(&PathBuf::from("a.png")), PreviewKind::Image);
        assert_eq!(detect_kind(&PathBuf::from("b.pdf")), PreviewKind::Pdf);
        assert_eq!(detect_kind(&PathBuf::from("c.rs")), PreviewKind::Text);
    }

    #[test]
    fn detects_more_text_extensions() {
        assert_eq!(detect_kind(&PathBuf::from("a.ts")), PreviewKind::Text);
        assert_eq!(detect_kind(&PathBuf::from("b.py")), PreviewKind::Text);
        assert_eq!(detect_kind(&PathBuf::from("c.go")), PreviewKind::Text);
    }

    #[test]
    fn unknown_extension() {
        assert_eq!(detect_kind(&PathBuf::from("a.bin")), PreviewKind::Unknown);
    }

    #[test]
    fn disabled_returns_error() {
        let cfg = FilePreviewConfig { enabled: false };
        let res = open_preview(std::path::Path::new("/dev/null"), &cfg);
        assert!(res.is_err());
    }
}
