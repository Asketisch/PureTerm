//! T-core-shaper:按字形选择字体的字体回退链。
//!
//! 回退链把 Unicode 文字区间映射到具体字面,
//! 使 CJK、emoji 与复杂文字无需单一巨型
//! 字体文件即可正确渲染。
//!
//! 默认链(来自规格说明):
//! 1. Monospace(主字体)——拉丁 / ASCII / 基本符号
//! 2. CJK JP——平假名、片假名、日文汉字
//! 3. CJK KR——谚文、韩文汉字
//! 4. CJK SC——简体中文
//! 5. Emoji——彩色 emoji(ZWJ 序列、修饰符)
//! 6. Serif——罕见字符的兜底

use std::sync::Arc;

use super::{rustybuzz_shaper::FontFace, RustyBuzzShaper, RustyBuzzConfig};

/// 回退链中的一个命名字面。
#[derive(Clone, Debug)]
pub struct NamedFontFace {
    /// 人类可读名称(如 "Monospace"、"CJK JP")。
    pub name: String,
    /// 实际字体数据。
    pub face: FontFace,
    /// 该字面针对的 Unicode 文字。
    pub script: FallbackScript,
}

/// 按优先级排列的受支持回退文字。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FallbackScript {
    /// 主等宽字体(拉丁、ASCII、基本符号)。
    Monospace,
    /// 日文字体(平假名、片假名、汉字)。
    CjkJp,
    /// 韩文字体(谚文、汉字)。
    CjkKr,
    /// 简体中文字体。
    CjkSc,
    /// 彩色 emoji 字体。
    Emoji,
    /// 罕见字符的衬线兜底。
    Serif,
}

impl FallbackScript {
    /// 取该文字的显示名。
    pub fn name(&self) -> &'static str {
        match self {
            Self::Monospace => "monospace",
            Self::CjkJp => "cjk-jp",
            Self::CjkKr => "cjk-kr",
            Self::CjkSc => "cjk-sc",
            Self::Emoji => "emoji",
            Self::Serif => "serif",
        }
    }

    /// 取该回退对应的 rustybuzz 文字标签。
    pub fn script_tag(&self) -> &'static str {
        match self {
            Self::Monospace => "latn",
            Self::CjkJp => "kana",
            Self::CjkKr => "hang",
            Self::CjkSc => "hani",
            Self::Emoji => "DFLT", // Emoji handled via variation selectors.
            Self::Serif => "DFLT",
        }
    }
}

/// 字体回退链。持有按优先级排序的命名字面列表,
/// 并提供为给定输入选择最佳字面的方法。
#[derive(Clone, Debug)]
pub struct FallbackChain {
    /// 有序的字面列表。
    pub faces: Vec<NamedFontFace>,
}

impl Default for FallbackChain {
    fn default() -> Self {
        // 构建不含实际字体的默认链(桩)。
        // 生产环境中应从系统字体或捆绑字体加载。
        Self { faces: Vec::new() }
    }
}

impl FallbackChain {
    /// 用给定的命名字面创建新的回退链。
    pub fn new(faces: Vec<NamedFontFace>) -> Self {
        Self { faces }
    }

    /// 取主(第一个)字面。用于拉丁/ASCII 文本。
    pub fn primary(&self) -> Option<&FontFace> {
        self.faces.first().map(|f| &f.face)
    }

    /// 按文字类型取字面。
    pub fn by_script(&self, script: FallbackScript) -> Option<&FontFace> {
        self.faces.iter()
            .find(|f| f.script == script)
            .map(|f| &f.face)
    }

    /// 从此回退链构建 RustyBuzzConfig。
    pub fn build_config(&self, language: String) -> RustyBuzzConfig {
        let primary = self.primary().cloned();
        let mut fallbacks = Vec::new();
        for face in self.faces.iter().skip(1) {
            fallbacks.push(face.face.clone());
        }

        RustyBuzzConfig {
            font_face: primary,
            fallback_faces: fallbacks,
            language,
            enable_features: true,
        }
    }

    /// 从此回退链创建整形器。
    pub fn create_shaper(&self, language: String) -> RustyBuzzShaper {
        let config = self.build_config(language);
        RustyBuzzShaper::new(config)
    }
}

/// 用特定字体数据构建回退链的 builder。
#[derive(Debug)]
pub struct FallbackChainBuilder {
    faces: Vec<NamedFontFace>,
    language: String,
}

impl Default for FallbackChainBuilder {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            language: "en-US".to_string(),
        }
    }
}

impl FallbackChainBuilder {
    /// 设置主等宽字体。
    pub fn with_monospace(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "Monospace".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::Monospace,
        });
        self
    }

    /// 添加 CJK JP 字体。
    pub fn with_cjk_jp(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "CJK JP".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::CjkJp,
        });
        self
    }

    /// 添加 CJK KR 字体。
    pub fn with_cjk_kr(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "CJK KR".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::CjkKr,
        });
        self
    }

    /// 添加 CJK SC 字体。
    pub fn with_cjk_sc(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "CJK SC".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::CjkSc,
        });
        self
    }

    /// 添加 emoji 字体。
    pub fn with_emoji(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "Emoji".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::Emoji,
        });
        self
    }

    /// 添加衬线兜底字体。
    pub fn with_serif(mut self, data: Arc<Vec<u8>>) -> Self {
        self.faces.push(NamedFontFace {
            name: "Serif".to_string(),
            face: FontFace::new(data),
            script: FallbackScript::Serif,
        });
        self
    }

    /// 设置整形所用语言。
    pub fn with_language(mut self, lang: String) -> Self {
        self.language = lang;
        self
    }

    /// 构建回退链。
    pub fn build(self) -> FallbackChain {
        FallbackChain::new(self.faces)
    }

    /// 直接构建整形器。
    pub fn create_shaper(self) -> RustyBuzzShaper {
        let language = self.language.clone();
        let chain = self.build();
        chain.create_shaper(language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shaper::Shaper;

    #[test]
    fn test_default_chain_is_empty() {
        let chain = FallbackChain::default();
        assert!(chain.faces.is_empty());
        assert!(chain.primary().is_none());
    }

    #[test]
    fn test_build_config_from_chain() {
        // 无字体时,配置的主字体应为 None、回退为空。
        let chain = FallbackChain::default();
        let config = chain.build_config("en-US".to_string());
        assert!(config.font_face.is_none());
        assert!(config.fallback_faces.is_empty());
        assert_eq!(config.language, "en-US");
    }

    #[test]
    fn test_by_script_returns_correct_face() {
        let data = Arc::new(vec![0u8; 100]); // dummy font data.
        let chain = FallbackChainBuilder::default()
            .with_monospace(data.clone())
            .with_cjk_jp(data.clone())
            .build();

        assert!(chain.by_script(FallbackScript::Monospace).is_some());
        assert!(chain.by_script(FallbackScript::CjkJp).is_some());
        assert!(chain.by_script(FallbackScript::Emoji).is_none());
    }

    #[test]
    fn test_script_names() {
        assert_eq!(FallbackScript::Monospace.name(), "monospace");
        assert_eq!(FallbackScript::CjkJp.name(), "cjk-jp");
        assert_eq!(FallbackScript::Emoji.name(), "emoji");
    }

    #[test]
    fn test_script_tags() {
        assert_eq!(FallbackScript::Monospace.script_tag(), "latn");
        assert_eq!(FallbackScript::CjkJp.script_tag(), "kana");
        assert_eq!(FallbackScript::CjkKr.script_tag(), "hang");
    }

    #[test]
    fn test_builder_creates_shaper() {
        // 即使没有真实字体,builder 也应产出合法整形器。
        let shaper = FallbackChainBuilder::default().create_shaper();
        let run = shaper.shape("hello", 7.0);
        assert_eq!(run.glyphs.len(), 5); // Monospace fallback.
    }
}
