//! 字体族名解析:把 CSS 风格的泛型名映射到当前平台可加载的真实族名。
//!
//! 背景:`[font].normal.family` 的默认值是 `"monospace"`(与 Alacritty
//! 一致的跨平台写法)。但 crossfont 的各后端对泛型名的支持并不一致:
//!
//! - **Linux / fontconfig**:原生理解 `monospace`、`sans-serif`、
//!   `serif` 等泛型名,直接传即可。
//! - **macOS / CoreText**:按*族名*精确查找,不认识泛型名。找不到就
//!   回退到 Menlo,并打印一条
//!   `Unable to load specified font monospace, falling back to Menlo`
//!   的 WARN —— 于是每次启动都会出现这条噪音,用户会误以为自己的
//!   字体配置坏了。
//! - **Windows / DirectWrite**:同样只认真实族名。
//!
//! 本模块在把族名交给 crossfont *之前*做一次平台化解析,让默认配置
//! 在各平台上都干净地加载到真正的等宽字体,而配置文件里仍可继续
//! 写可移植的泛型名。

/// 把泛型字体族名解析为当前平台可加载的真实族名。
///
/// 非泛型名(用户显式指定的字体,如 `"JetBrains Mono"`)原样返回。
pub fn resolve(family: &str) -> String {
    let trimmed = family.trim();
    if trimmed.is_empty() {
        return platform_default_monospace().to_string();
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "monospace" | "ui-monospace" | "mono" => platform_default_monospace().to_string(),
        "sans-serif" | "ui-sans-serif" => platform_default_sans_serif().to_string(),
        "serif" => platform_default_serif().to_string(),
        _ => family.to_string(),
    }
}

/// 平台默认等宽字体族名。
pub fn platform_default_monospace() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Menlo"
    }
    #[cfg(target_os = "windows")]
    {
        "Consolas"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // fontconfig 理解泛型名,保持原样即可。
        "monospace"
    }
}

/// 平台默认无衬线字体族名。
pub fn platform_default_sans_serif() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Helvetica"
    }
    #[cfg(target_os = "windows")]
    {
        "Segoe UI"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "sans-serif"
    }
}

/// 平台默认衬线字体族名。
pub fn platform_default_serif() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Times New Roman"
    }
    #[cfg(target_os = "windows")]
    {
        "Times New Roman"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "serif"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_names_resolve_to_real_families() {
        // 解析后不应再是泛型名 —— 但仅限不认识泛型名的平台
        // (macOS/CoreText、Windows/DirectWrite)。Linux/fontconfig
        // 原生理解 "monospace",platform_default_monospace() 在
        // Linux 上刻意保持原样,故此处按平台分别断言。
        let mono = resolve("monospace");
        assert!(!mono.is_empty());
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            assert_ne!(mono.to_ascii_lowercase(), "monospace", "macOS/Windows 应解析为真实族名");
        } else {
            assert_eq!(mono.to_ascii_lowercase(), "monospace", "fontconfig 保持泛型名");
        }
    }

    #[test]
    fn explicit_families_pass_through() {
        assert_eq!(resolve("JetBrains Mono"), "JetBrains Mono");
        assert_eq!(resolve("Fira Code"), "Fira Code");
    }

    #[test]
    fn case_insensitive_generic_matching() {
        assert_eq!(resolve("Monospace"), resolve("monospace"));
        assert_eq!(resolve("MONOSPACE"), resolve("monospace"));
    }

    #[test]
    fn empty_family_uses_platform_default() {
        assert_eq!(resolve("   "), platform_default_monospace());
    }

    #[test]
    fn platform_defaults_are_non_empty() {
        assert!(!platform_default_monospace().is_empty());
        assert!(!platform_default_sans_serif().is_empty());
        assert!(!platform_default_serif().is_empty());
    }
}
