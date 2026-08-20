//! GAP-07:Soul 系统——加载 SOUL.md / STYLE.md / SKILL.md / MEMORY.md,
//! 注入 AI 聊天系统提示词。

use std::path::{Path, PathBuf};

use crate::app_config::SoulConfig;

const FILES: [(&str, &str); 4] = [
    ("Personality (SOUL)", "SOUL.md"),
    ("Style", "STYLE.md"),
    ("Skills", "SKILL.md"),
    ("Memory", "MEMORY.md"),
];

/// 限制每个 soul 文件的注入量,使数 MB 的大文件(如无上限的
/// MEMORY.md)不会在每次聊天轮次全量携带。约 32 KiB 对
/// 人格/风格/技能/记忆笔记已足够。
const MAX_SOUL_FILE_BYTES: usize = 32_768;

/// 默认目录:`~/.config/pureterm/ai/`。
pub fn default_directory() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pureterm")
        .join("ai")
}

/// 从配置解析 soul 文件目录。
pub fn resolve_directory(cfg: &SoulConfig) -> PathBuf {
    cfg.directory
        .clone()
        .unwrap_or_else(default_directory)
}

/// 加载并拼接各 soul markdown 文件,作为系统提示词的扩展。
pub fn load_system_prompt_extension(cfg: &SoulConfig) -> String {
    if !cfg.enabled {
        return String::new();
    }
    load_from_directory(&resolve_directory(cfg))
}

/// 从显式目录加载 soul 文件(供测试使用)。
pub fn load_from_directory(dir: &Path) -> String {
    let mut parts = Vec::new();
    for (label, file) in FILES {
        let path = dir.join(file);
        if let Ok(mut content) = std::fs::read_to_string(&path) {
            if content.len() > MAX_SOUL_FILE_BYTES {
                // 先截断到字节上限,再回退到 UTF-8 字符边界
                // (`truncate` 可能切开多字节序列),并附上
                // 截断标记,让模型知道文件被省略了。
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
        // 一个远超上限的文件。
        fs::write(dir.join("MEMORY.md"), "X".repeat(MAX_SOUL_FILE_BYTES + 5000)).unwrap();
        let out = load_from_directory(&dir);
        // 截断标记确实存在…
        assert!(out.contains("[truncated]"));
        // …且加载的内容最多保留 MAX_SOUL_FILE_BYTES 字节负载。
        assert!(out.matches('X').count() <= MAX_SOUL_FILE_BYTES);
        let _ = fs::remove_dir_all(&dir);
    }
}
