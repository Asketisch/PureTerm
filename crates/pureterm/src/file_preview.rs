//! GAP-34: File preview — image/PDF/text path detection + open helper.

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
        "txt" | "md" | "rs" | "toml" | "json" | "yaml" | "yml" | "log" => PreviewKind::Text,
        _ => PreviewKind::Unknown,
    }
}

/// Open preview via platform handler. MVP: `open` / `xdg-open`.
pub fn open_preview(path: &Path, cfg: &FilePreviewConfig) -> Result<(), String> {
    if !cfg.enabled {
        return Err("file_preview disabled".into());
    }
    if !path.exists() {
        return Err(format!("path not found: {}", path.display()));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .status()
            .map_err(|e| e.to_string())?;
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
}
