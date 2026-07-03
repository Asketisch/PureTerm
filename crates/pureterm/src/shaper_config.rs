//! T-core-shaper v2: configuration for the shaper backend.
//!
//! Mirrors the `[shaper]` block in `default-config.toml`. The shaper
//! defaults to `monospace` (the v1 cluster-aware shaper) and v2 swaps in
//! `rustybuzz` when the user opts in and provides a font blob.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Which shaper backend to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShaperBackend {
    /// Pure-Rust cluster-aware shaper (v1). Always available, no
    /// font-load dependency. Correct for Latin + CJK width; doesn't
    /// handle ligatures or complex script shaping.
    Monospace,
    /// Harfbuzz via rustybuzz (v2). Requires a real font blob and the
    /// `rustybuzz` / `unicode-bidi` / `unicode-script` deps, which are
    /// already pulled in by `pureterm-alacritty-runtime`.
    RustyBuzz,
}

impl Default for ShaperBackend {
    fn default() -> Self {
        Self::Monospace
    }
}

impl ShaperBackend {
    pub fn parse(s: &str) -> Self {
        match s {
            "rustybuzz" | "harfbuzz" | "hb" => Self::RustyBuzz,
            _ => Self::Monospace,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Monospace => "monospace",
            Self::RustyBuzz => "rustybuzz",
        }
    }
}

/// The fallback chain entries the user can put in `[shaper].fallback_chain`.
/// Each entry names a font file and the script the font covers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackEntry {
    /// Display name (informational).
    pub name: String,
    /// Path to a .ttf / .otf / .ttc file. May be `~`-expanded by the
    /// loader.
    pub path: String,
    /// Script tag: "cjk-jp" / "cjk-kr" / "cjk-sc" / "emoji" / "serif".
    pub script: String,
}

/// The shaper configuration block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShaperConfig {
    pub backend: ShaperBackend,
    /// ISO-639 language tag for script-specific features (e.g. "en-US",
    /// "ja", "zh-Hans"). Defaults to "en-US".
    pub language: String,
    /// Whether to enable OpenType features (ligatures, contextual forms).
    pub enable_features: bool,
    /// Ordered fallback font chain. Used when the primary font is missing
    /// a glyph.
    pub fallback_chain: Vec<FallbackEntry>,
}

impl Default for ShaperConfig {
    fn default() -> Self {
        Self {
            backend: ShaperBackend::Monospace,
            language: "en-US".to_string(),
            enable_features: true,
            fallback_chain: vec![
                FallbackEntry {
                    name: "Monospace".to_string(),
                    path: String::new(),
                    script: "monospace".to_string(),
                },
                FallbackEntry {
                    name: "CJK JP".to_string(),
                    path: String::new(),
                    script: "cjk-jp".to_string(),
                },
                FallbackEntry {
                    name: "CJK KR".to_string(),
                    path: String::new(),
                    script: "cjk-kr".to_string(),
                },
                FallbackEntry {
                    name: "CJK SC".to_string(),
                    path: String::new(),
                    script: "cjk-sc".to_string(),
                },
                FallbackEntry {
                    name: "Emoji".to_string(),
                    path: String::new(),
                    script: "emoji".to_string(),
                },
                FallbackEntry {
                    name: "Serif".to_string(),
                    path: String::new(),
                    script: "serif".to_string(),
                },
            ],
        }
    }
}

impl ShaperConfig {
    /// Load the `[shaper]` block from a TOML config file. Falls back to
    /// the default if the block is absent or any field is missing.
    pub fn load(path: &Path) -> Self {
        let mut cfg = Self::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            return cfg;
        };
        // toml_edit is the spec-compliant parser we already depend on for
        // the config IO layer (preserves formatting for `pureterm-cli config
        // set`). It happily accepts a partial document like
        // `[shaper]\nbackend = "rustybuzz"\n` without complaining about
        // missing root-level fields.
        let Ok(value) = text.parse::<toml_edit::DocumentMut>() else {
            return cfg;
        };
        // Walk to the [shaper] table.
        let Some(table) = value
            .as_item()
            .get("shaper")
            .and_then(|i| i.as_table())
        else {
            return cfg;
        };
        if let Some(s) = table.get("backend").and_then(|v| v.as_str()) {
            cfg.backend = ShaperBackend::parse(s);
        }
        if let Some(s) = table.get("language").and_then(|v| v.as_str()) {
            cfg.language = s.to_string();
        }
        if let Some(b) = table.get("enable_features").and_then(|v| v.as_bool()) {
            cfg.enable_features = b;
        }
        if let Some(arr) = table.get("fallback_chain").and_then(|v| v.as_array()) {
            let mut entries = Vec::new();
            // In toml_edit 0.22, `Array` is heterogeneous and stores
            // `Item` values. Inline tables arrive as `Item::Value(Value::InlineTable(...))`
            // — unwrap them and pull each entry's keys via `get` on the
            // inline table.
            for item in arr.iter() {
                let inline = match item.as_inline_table() {
                    Some(t) => t,
                    None => continue,
                };
                let name = inline
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let path = inline
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let script = inline
                    .get("script")
                    .and_then(|v| v.as_str())
                    .unwrap_or("monospace")
                    .to_string();
                entries.push(FallbackEntry { name, path, script });
            }
            if !entries.is_empty() {
                cfg.fallback_chain = entries;
            }
        }
        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_monospace() {
        let cfg = ShaperConfig::default();
        assert_eq!(cfg.backend, ShaperBackend::Monospace);
        assert_eq!(cfg.language, "en-US");
        assert!(cfg.enable_features);
        // Default chain has 6 entries per the spec.
        assert_eq!(cfg.fallback_chain.len(), 6);
    }

    #[test]
    fn parse_backend_string() {
        assert_eq!(ShaperBackend::parse("monospace"), ShaperBackend::Monospace);
        assert_eq!(ShaperBackend::parse("rustybuzz"), ShaperBackend::RustyBuzz);
        assert_eq!(ShaperBackend::parse("hb"), ShaperBackend::RustyBuzz);
        assert_eq!(ShaperBackend::parse("garbage"), ShaperBackend::Monospace);
    }

    #[test]
    fn backend_as_str_roundtrips() {
        assert_eq!(ShaperBackend::Monospace.as_str(), "monospace");
        assert_eq!(ShaperBackend::RustyBuzz.as_str(), "rustybuzz");
    }

    #[test]
    fn load_missing_file_returns_default() {
        let cfg = ShaperConfig::load(Path::new("/does/not/exist.toml"));
        assert_eq!(cfg, ShaperConfig::default());
    }

    #[test]
    fn load_partial_block_uses_defaults_for_missing_fields() {
        let dir = std::env::temp_dir().join("pureterm-shaper-config-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("partial.toml");
        std::fs::write(&path, "[shaper]\nbackend = \"rustybuzz\"\n").unwrap();
        let cfg = ShaperConfig::load(&path);
        assert_eq!(cfg.backend, ShaperBackend::RustyBuzz);
        assert_eq!(cfg.language, "en-US");
        assert!(cfg.enable_features);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_full_block() {
        let dir = std::env::temp_dir().join("pureterm-shaper-config-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("full.toml");
        let toml = r#"
[shaper]
backend = "rustybuzz"
language = "ja"
enable_features = false
fallback_chain = [
    { name = "Mono", path = "/tmp/a.ttf", script = "monospace" },
    { name = "JP", path = "/tmp/b.ttf", script = "cjk-jp" },
]
"#;
        std::fs::write(&path, toml).unwrap();
        let cfg = ShaperConfig::load(&path);
        assert_eq!(cfg.backend, ShaperBackend::RustyBuzz);
        assert_eq!(cfg.language, "ja");
        assert!(!cfg.enable_features);
        assert_eq!(cfg.fallback_chain.len(), 2);
        assert_eq!(cfg.fallback_chain[0].name, "Mono");
        assert_eq!(cfg.fallback_chain[1].script, "cjk-jp");
        std::fs::remove_file(&path).ok();
    }
}
