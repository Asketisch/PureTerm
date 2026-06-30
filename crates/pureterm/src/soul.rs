//! GAP-07: Soul System — load SOUL.md / STYLE.md / SKILL.md / MEMORY.md and
//! inject into AI Chat system prompts.

use std::path::{Path, PathBuf};

use crate::app_config::SoulConfig;

const FILES: [(&str, &str); 4] = [
    ("Personality (SOUL)", "SOUL.md"),
    ("Style", "STYLE.md"),
    ("Skills", "SKILL.md"),
    ("Memory", "MEMORY.md"),
];

/// Cap each soul file's contribution so a multi-MB file (e.g. an unbounded
/// MEMORY.md) isn't shipped on every chat turn. ~32 KiB is ample for
/// personality/style/skill/memory notes.
const MAX_SOUL_FILE_BYTES: usize = 32_768;

/// Default directory: `~/.config/pureterm/ai/`.
pub fn default_directory() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pureterm")
        .join("ai")
}

/// Resolve the soul file directory from config.
pub fn resolve_directory(cfg: &SoulConfig) -> PathBuf {
    cfg.directory
        .clone()
        .unwrap_or_else(default_directory)
}

/// Load and concatenate soul markdown files into a system-prompt extension.
pub fn load_system_prompt_extension(cfg: &SoulConfig) -> String {
    if !cfg.enabled {
        return String::new();
    }
    load_from_directory(&resolve_directory(cfg))
}

/// Load soul files from an explicit directory (for tests).
pub fn load_from_directory(dir: &Path) -> String {
    let mut parts = Vec::new();
    for (label, file) in FILES {
        let path = dir.join(file);
        if let Ok(mut content) = std::fs::read_to_string(&path) {
            if content.len() > MAX_SOUL_FILE_BYTES {
                // Truncate to the byte cap, then snap back to a UTF-8 char
                // boundary (`truncate` may split a multi-byte sequence) and note
                // the cut so the model knows the file was elided.
                content.truncate(MAX_SOUL_FILE_BYTES);
                while !content.is_char_boundary(content.len()) {
                    content.pop();
                }
                content.push_str("\n…[truncated]");
            }
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                parts.push(format!("## {label}\n{trimmed}"));
            }
        }
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn disabled_returns_empty() {
        let cfg = SoulConfig {
            enabled: false,
            directory: None,
        };
        assert!(load_system_prompt_extension(&cfg).is_empty());
    }

    #[test]
    fn loads_present_files() {
        let dir = std::env::temp_dir().join(format!("pureterm-soul-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SOUL.md"), "You are a Rust expert.").unwrap();
        fs::write(dir.join("STYLE.md"), "Be concise.").unwrap();
        let out = load_from_directory(&dir);
        assert!(out.contains("Rust expert"));
        assert!(out.contains("Be concise"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_soul_file_is_truncated() {
        let dir = std::env::temp_dir().join(format!("pureterm-soul-cap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // A file well over the cap.
        fs::write(dir.join("MEMORY.md"), "X".repeat(MAX_SOUL_FILE_BYTES + 5000)).unwrap();
        let out = load_from_directory(&dir);
        // The cap marker is present…
        assert!(out.contains("[truncated]"));
        // …and the loaded content kept at most MAX_SOUL_FILE_BYTES payload bytes.
        assert!(out.matches('X').count() <= MAX_SOUL_FILE_BYTES);
        let _ = fs::remove_dir_all(&dir);
    }
}
