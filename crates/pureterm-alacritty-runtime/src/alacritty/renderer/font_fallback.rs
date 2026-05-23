//! 字体回退链：当主字体不支持某个字符时，通过 fontdb 发现系统字体。
//!
//! 5 步回退链：
//! 1. 缓存命中（已解析过的码点）
//! 2. 用户配置的 fallback_families
//! 3. Emoji 专用字体
//! 4. CJK 专用字体（平台特定）
//! 5. 通用兜底（SansSerif → Serif）

use std::collections::HashMap;

use crossfont::{FontDesc, FontKey, Rasterize, Rasterizer, Size, Slant, Style, Weight};
use fontdb::Database;

/// 字体回退注册表。管理 codepoint → FontKey 的映射。
pub struct FontFallbackRegistry {
    db: Database,
    initialized: bool,
    /// 码点 → atlas font_index
    char_to_index: HashMap<u32, usize>,
    /// atlas font_index → crossfont FontKey
    index_to_key: HashMap<usize, FontKey>,
    /// 已知 family_name → atlas font_index（避免重复加载）
    family_to_index: HashMap<String, usize>,
    /// 下一个可用的 atlas font_index（0 保留给主字体）
    next_index: usize,
    /// 用户配置的回退字体族列表
    fallback_families: Vec<String>,
}

impl FontFallbackRegistry {
    pub fn new(fallback_families: Vec<String>) -> Self {
        Self {
            db: Database::new(),
            initialized: false,
            char_to_index: HashMap::new(),
            index_to_key: HashMap::new(),
            family_to_index: HashMap::new(),
            next_index: 1,
            fallback_families,
        }
    }

    fn ensure_initialized(&mut self) {
        if !self.initialized {
            self.db.load_system_fonts();
            log::debug!(
                "fontdb initialized with {} system fonts",
                self.db.faces().count()
            );
            self.initialized = true;
        }
    }

    /// 为给定字符查找回退字体，返回 atlas font_index。
    pub fn resolve(
        &mut self,
        c: char,
        rasterizer: &mut Rasterizer,
        font_size: Size,
    ) -> Option<usize> {
        let cp = c as u32;

        // Step 1: Cache hit
        if let Some(&idx) = self.char_to_index.get(&cp) {
            return Some(idx);
        }

        self.ensure_initialized();

        // Step 2: User-configured families
        let user_families = self.fallback_families.clone();
        for family in &user_families {
            if let Some(idx) = self.try_family(family, c, rasterizer, font_size) {
                self.char_to_index.insert(cp, idx);
                return Some(idx);
            }
        }

        // Step 3: Emoji
        if is_emoji(c) {
            for name in EMOJI_FAMILIES {
                if let Some(idx) = self.try_family(name, c, rasterizer, font_size) {
                    self.char_to_index.insert(cp, idx);
                    return Some(idx);
                }
            }
        }

        // Step 4: CJK
        if is_cjk(c) {
            for name in CJK_FAMILIES {
                if let Some(idx) = self.try_family(name, c, rasterizer, font_size) {
                    self.char_to_index.insert(cp, idx);
                    return Some(idx);
                }
            }
        }

        // Step 5: Generic fallback
        for name in &["Sans-Serif", "Serif"] {
            if let Some(idx) = self.try_family(name, c, rasterizer, font_size) {
                self.char_to_index.insert(cp, idx);
                return Some(idx);
            }
        }

        // Mark as unresolved to avoid repeated expensive lookups
        None
    }

    /// 尝试从指定字体族加载字体并验证它支持给定字符。
    fn try_family(
        &mut self,
        family_name: &str,
        c: char,
        rasterizer: &mut Rasterizer,
        font_size: Size,
    ) -> Option<usize> {
        // Fast path: already loaded this family
        if let Some(&idx) = self.family_to_index.get(family_name) {
            // Verify the loaded font supports this codepoint via atlas cache
            if self.char_to_index.values().any(|&v| v == idx) {
                // This family was loaded before, try rasterizing the glyph
                let font_key = self.index_to_key.get(&idx)?;
                let glyph_key = crossfont::GlyphKey {
                    character: c,
                    font_key: *font_key,
                    size: font_size,
                };
                if rasterizer.get_glyph(glyph_key).is_ok() {
                    return Some(idx);
                }
            }
        }

        // Find a face in this family that supports the codepoint
        let face_id = self.find_face_for_codepoint(family_name, c)?;

        // Get family name from the matched face
        let resolved_family = self.get_face_family(face_id)?;

        // Load into crossfont
        let desc = FontDesc::new(
            resolved_family.clone(),
            Style::Description {
                slant: Slant::Normal,
                weight: Weight::Normal,
            },
        );
        let font_key = rasterizer.load_font(&desc, font_size).ok()?;

        let idx = self.next_index;
        self.next_index += 1;

        self.index_to_key.insert(idx, font_key);
        self.family_to_index.insert(resolved_family, idx);

        log::debug!(
            "fallback: U+{:04X} '{}' -> family '{}' (atlas index {})",
            c as u32, c, family_name, idx
        );

        Some(idx)
    }

    /// 在 fontdb 中查找指定族中支持给定码点的字体 face。
    fn find_face_for_codepoint(&self, family_name: &str, c: char) -> Option<fontdb::ID> {
        // fontdb query returns the best match; we check all faces in the family.
        for face_info in self.db.faces() {
            let matches_family = face_info.families.iter().any(|(fam, _)| fam == family_name);
            if !matches_family {
                continue;
            }
            if self.face_has_codepoint(face_info.id, c) {
                return Some(face_info.id);
            }
        }
        None
    }

    /// 检查 face 是否支持给定码点。
    fn face_has_codepoint(&self, face_id: fontdb::ID, c: char) -> bool {
        let (source, index) = match self.db.face_source(face_id) {
            Some(s) => s,
            None => return false,
        };

        let data: Option<Vec<u8>> = match source {
            fontdb::Source::File(ref path) => std::fs::read(path).ok(),
            fontdb::Source::Binary(ref bytes) => Some(bytes.as_ref().as_ref().to_vec()),
            fontdb::Source::SharedFile(ref path, _) => std::fs::read(path).ok(),
        };

        let data = match data {
            Some(d) => d,
            None => return false,
        };

        ttf_parser::Face::parse(&data, index)
            .ok()
            .and_then(|face| face.glyph_index(c))
            .is_some()
    }

    /// 获取 face 的英文字体族名。
    fn get_face_family(&self, face_id: fontdb::ID) -> Option<String> {
        let face_info = self.db.face(face_id)?;
        face_info
            .families
            .first()
            .map(|(name, _)| name.clone())
            .or(Some(face_info.post_script_name.clone()))
    }

    /// 获取指定 atlas index 对应的 crossfont FontKey。
    pub fn font_key_for_index(&self, index: usize) -> Option<FontKey> {
        self.index_to_key.get(&index).copied()
    }

    /// 字号变化时清除缓存（FontKey 仍然有效，但需要重新光栅化）。
    pub fn clear_char_cache(&mut self) {
        self.char_to_index.clear();
    }
}

// ── Emoji / CJK 检测 ────────────────────────────────────────────────────

fn is_emoji(c: char) -> bool {
    let cp = c as u32;
    in_range(cp, 0x1F600, 0x1F64F)
        || in_range(cp, 0x1F300, 0x1F5FF)
        || in_range(cp, 0x1F680, 0x1F6FF)
        || in_range(cp, 0x1F900, 0x1F9FF)
        || in_range(cp, 0x1FA00, 0x1FA6F)
        || in_range(cp, 0x1FA70, 0x1FAFF)
        || in_range(cp, 0x2600, 0x26FF)
        || in_range(cp, 0x2700, 0x27BF)
        || in_range(cp, 0xFE00, 0xFE0F)
        || in_range(cp, 0x1F000, 0x1F02F)
        || in_range(cp, 0x1F0A0, 0x1F0FF)
        || in_range(cp, 0x231A, 0x231B)
        || in_range(cp, 0x23E9, 0x23F3)
        || in_range(cp, 0x23F8, 0x23FA)
        || in_range(cp, 0x25AA, 0x25AB)
        || in_range(cp, 0x25B6, 0x25C0)
        || in_range(cp, 0x25FB, 0x25FE)
        || in_range(cp, 0x2614, 0x2615)
        || in_range(cp, 0x2648, 0x2653)
        || cp == 0x267F
        || cp == 0x2693
        || cp == 0x26A1
        || in_range(cp, 0x26AA, 0x26AB)
        || in_range(cp, 0x26BD, 0x26BE)
        || in_range(cp, 0x26C4, 0x26C5)
        || in_range(cp, 0x26CE, 0x26CF)
        || cp == 0x26D4
        || in_range(cp, 0x26EA, 0x26EB)
        || in_range(cp, 0x26F0, 0x26F5)
        || cp == 0x26FA
        || cp == 0x26FD
}

fn is_cjk(c: char) -> bool {
    let cp = c as u32;
    in_range(cp, 0x4E00, 0x9FFF)
        || in_range(cp, 0x3400, 0x4DBF)
        || in_range(cp, 0x3000, 0x303F)
        || in_range(cp, 0x3040, 0x309F)
        || in_range(cp, 0x30A0, 0x30FF)
        || in_range(cp, 0xAC00, 0xD7AF)
        || in_range(cp, 0x2E80, 0x2EFF)
        || in_range(cp, 0xF900, 0xFAFF)
        || in_range(cp, 0x20000, 0x2A6DF)
        || in_range(cp, 0x2F800, 0x2FA1F)
        || in_range(cp, 0x17000, 0x187FF)
        || in_range(cp, 0x18B00, 0x18BFF)
}

#[inline]
fn in_range(cp: u32, lo: u32, hi: u32) -> bool {
    cp >= lo && cp <= hi
}

/// Emoji 字体族（按优先级排序）。
const EMOJI_FAMILIES: &[&str] = &[
    "Apple Color Emoji",
    "Segoe UI Emoji",
    "Noto Color Emoji",
    "Twitter Color Emoji",
    "EmojiOne Color",
    "Symbols Nerd Font",
];

/// CJK 字体族（按平台排序）。
#[cfg(target_os = "macos")]
const CJK_FAMILIES: &[&str] = &[
    "PingFang SC",
    "PingFang TC",
    "PingFang HK",
    "Hiragino Sans GB",
    "Songti SC",
    "STHeiti",
    "Microsoft YaHei",
    "SimHei",
];

#[cfg(target_os = "linux")]
const CJK_FAMILIES: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans CJK TC",
    "Noto Sans CJK JP",
    "WenQuanYi Micro Hei",
    "Source Han Sans",
    "AR PL UMing CN",
];

#[cfg(target_os = "windows")]
const CJK_FAMILIES: &[&str] = &[
    "Microsoft YaHei",
    "SimHei",
    "Source Han Sans SC",
    "Noto Sans CJK SC",
];

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
const CJK_FAMILIES: &[&str] = &[];
