//! GAP-59:emoji 专用渲染通道。
//!
//! 为 emoji 字形使用独立的彩色 atlas(macOS 上为 Apple Color Emoji,
//! Linux 上为 Noto Color Emoji)。atlas 是 Rgba8UnormSrgb 的 wgpu
//! 纹理;字形经 `crossfont` 光栅化并按需上传。
//! `is_complex_emoji` 检测 ZWJ 序列,调用方可将此类 emoji
//! 路由到专用路径(而非灰度文本通道)。

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmojiRenderingConfig {
    pub enabled: bool,
    pub dedicated_atlas: bool,
    pub font_fallback: String,
    /// 可选的第二回退字体(如 Linux 上的 "Noto Color Emoji")。
    pub font_fallback_secondary: String,
}

impl Default for EmojiRenderingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dedicated_atlas: false,
            font_fallback: "Apple Color Emoji".into(),
            font_fallback_secondary: "Noto Color Emoji".into(),
        }
    }
}

impl EmojiRenderingConfig {
    pub fn use_dedicated_channel(&self) -> bool {
        self.enabled && self.dedicated_atlas
    }

    /// 按顺序返回待尝试字体的迭代器。始终非空;
    /// 操作系统/平台默认字体是第一个条目。
    pub fn fallback_chain(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.font_fallback.as_str())
            .chain(std::iter::once(self.font_fallback_secondary.as_str()))
            .filter(|s| !s.is_empty())
    }

    /// 检测 ZWJ emoji 序列(基础 MVP 启发式)。
    pub fn is_complex_emoji(s: &str) -> bool {
        s.contains('\u{200D}') || s.chars().count() > 1
    }
}

/// 已光栅化、可上传到彩色 atlas 的 emoji 字形。
/// `width` / `height` 单位为像素;`data` 为 RGBA8(预乘
/// alpha,与 `crossfont::BitmapBuffer::Rgba` 产出一致)。
#[derive(Clone, Debug)]
pub struct EmojiGlyph {
    pub character: char,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl EmojiGlyph {
    /// 当且仅当字形至少有一个非透明像素时返回 true。
    pub fn has_visible_content(&self) -> bool {
        // RGBA:跳过每 4 个字节中的第 4 个(alpha 通道)。一个
        // 全透明像素的字形在所有条目上都会表现为零 alpha,
        // 这会被 `any` 捕获。
        self.data.chunks_exact(4).any(|c| c[3] != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zwj_detected() {
        assert!(EmojiRenderingConfig::is_complex_emoji("👨‍💻"));
    }

    #[test]
    fn single_codepoint_not_complex() {
        assert!(!EmojiRenderingConfig::is_complex_emoji("a"));
    }

    #[test]
    fn fallback_chain_skips_empty() {
        let cfg = EmojiRenderingConfig {
            enabled: true,
            dedicated_atlas: false,
            font_fallback: "Apple Color Emoji".into(),
            font_fallback_secondary: String::new(),
        };
        let names: Vec<&str> = cfg.fallback_chain().collect();
        assert_eq!(names, vec!["Apple Color Emoji"]);
    }

    #[test]
    fn fallback_chain_ordered() {
        let cfg = EmojiRenderingConfig {
            enabled: true,
            dedicated_atlas: false,
            font_fallback: "Apple Color Emoji".into(),
            font_fallback_secondary: "Noto Color Emoji".into(),
        };
        let names: Vec<&str> = cfg.fallback_chain().collect();
        assert_eq!(names, vec!["Apple Color Emoji", "Noto Color Emoji"]);
    }

    #[test]
    fn dedicated_channel_requires_both() {
        let mut cfg = EmojiRenderingConfig::default();
        assert!(!cfg.use_dedicated_channel());
        cfg.dedicated_atlas = true;
        assert!(cfg.use_dedicated_channel());
        cfg.enabled = false;
        assert!(!cfg.use_dedicated_channel());
    }

    #[test]
    fn emoji_glyph_visibility_check() {
        // 2x2 RGBA,全部透明。
        let empty = EmojiGlyph {
            character: ' ',
            width: 2,
            height: 2,
            data: vec![0u8; 16],
        };
        assert!(!empty.has_visible_content());
        // 只有一个不透明像素。
        let mut visible = empty.clone();
        visible.data[3] = 255;
        assert!(visible.has_visible_content());
    }
}
