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
        if let Ok(content) = std::fs::read_to_string(&path) {
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
}
