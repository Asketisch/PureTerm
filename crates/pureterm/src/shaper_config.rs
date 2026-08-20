//! T-core-shaper v2: 字形整形后端的配置。
//!
//! 对应 `default-config.toml` 中的 `[shaper]` 配置块。整形后端
//! 默认使用 `monospace`(v1 聚簇感知整形器);用户选择 v2 时切换为
//! `rustybuzz`,前提是用户提供字体 blob。

use std::path::Path;

use serde::{Deserialize, Serialize};

/// 使用哪个字形整形后端。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShaperBackend {
    /// 纯 Rust 聚簇感知整形器(v1)。始终可用,不依赖
    /// 字体文件加载。对拉丁文与中日韩文字宽度处理正确;
    /// 但不支持连字与复杂文字整形。
    Monospace,
    /// 通过 rustybuzz 调用 Harfbuzz(v2)。需要真实的字体 blob,以及
    /// `rustybuzz` / `unicode-bidi` / `unicode-script` 依赖,这些依赖
    /// 已由 `pureterm-alacritty-runtime` 引入。
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

/// 用户可放入 `[shaper].fallback_chain` 的回退字体链条目。
/// 每个条目指定一个字体文件路径,以及该字体覆盖的文字脚本。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackEntry {
    /// 显示名称(仅供参考)。
    pub name: String,
    /// 指向 .ttf / .otf / .ttc 文件的路径。可能由加载器执行 `~` 展开。
    /// 负责展开处理。
    pub path: String,
    /// 文字脚本标签: "cjk-jp" / "cjk-kr" / "cjk-sc" / "emoji" / "serif"。
    pub script: String,
}

/// 字形整形配置块。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShaperConfig {
    pub backend: ShaperBackend,
    /// 用于文字特性选择的 ISO-639 语言标签(例如 "en-US",
    /// "ja"、"zh-Hans")。默认值为 "en-US"。
    pub language: String,
    /// 是否启用 OpenType 特性(连字、上下文替换等)。
    pub enable_features: bool,
    /// 有序的回退字体链。当主字体缺失字形时
    /// 使用回退。
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
    /// 从 TOML 配置文件加载 `[shaper]` 配置块。若块不存在或
    /// 字段缺失,则回退到默认配置。
    pub fn load(path: &Path) -> Self {
        let mut cfg = Self::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            return cfg;
        };
        // toml_edit 是符合规范的解析器,配置 IO 层已依赖它来
        // 保留格式(供 `pureterm-cli config` 使用)。它能接受残缺文档,
        // 它能接受形如 `[shaper]\nbackend = "rustybuzz"\n` 的残缺文档,而不会
        // 抱怨缺少顶层字段。
        let Ok(value) = text.parse::<toml_edit::DocumentMut>() else {
            return cfg;
        };
        // 定位到 [shaper] 配置表。
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
            // 在 toml_edit 0.22 中,`Array` 是异构的,存储
            // `Item` 值。内联表以 `Item::Value(Value::InlineTable(...))` 形式到达,
            // 需解包,并通过 `get` 获取每个条目的键(见第 153 行)。
            //
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
        // 按规格说明,默认回退链有 6 个条目。
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
