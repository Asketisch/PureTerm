//! 使用 fontdb 的字体回退链解析器。
//!
//! 5 步解析：
//! 1. 主字体（用户配置的）— 由 GlyphAtlas 处理
//! 2. 家族回退（匹配码点的系统字体）
//! 3. Emoji 回退（Apple Color Emoji、Noto Color Emoji 等）
//! 4. CJK 回退（Noto Sans CJK、PingFang 等）
//! 5. 最后手段（任何可用字体）

use fontdb::{Database, Family, Query};
use std::path::PathBuf;

/// 用于回退的已识别字体，已加载其原始数据。
#[derive(Debug, Clone)]
pub struct FallbackFont {
    /// 字体文件的完整路径。
    pub path: PathBuf,
    /// 字体文件中的字面索引（用于 TTC/OTC 集合）。
    pub face_index: u32,
    /// 原始字体数据字节。
    pub data: Vec<u8>,
    /// 此字体是否有彩色字形（emoji）。
    pub is_color: bool,
}

/// 使用 fontdb 的字体回退解析器。
pub struct FontFallback {
    db: Database,
    /// 按字体路径加载的字体数据（用于去重）。
    loaded_fonts: std::collections::HashMap<PathBuf, Vec<u8>>,
}

impl FontFallback {
    /// 创建新的回退解析器并扫描系统字体。
    pub fn new() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        Self {
            db,
            loaded_fonts: std::collections::HashMap::new(),
        }
    }

    /// 为给定码点查找回退字体。
    /// 返回已完全加载的字体及其数据。
    pub fn find(&mut self, codepoint: char) -> Option<FallbackFont> {
        // Step 2: Try family fallback (system monospace)
        // Verify the font actually contains a glyph for this codepoint
        if let Some(f) = self.try_family_for_codepoint(&Family::Monospace, codepoint) {
            return Some(f);
        }

        // Step 3: Emoji fallback
        if is_emoji(codepoint) {
            for name in EMOJI_FAMILIES {
                if let Some(f) = self.try_family_for_codepoint(&Family::Name(name), codepoint) {
                    return Some(f);
                }
            }
        }

        // Step 4: CJK fallback
        if is_cjk(codepoint) {
            #[cfg(target_os = "macos")]
            {
                if let Some(f) = self.try_family_for_codepoint(&Family::Name("Apple Color Emoji"), codepoint) {
                    return Some(f);
                }
            }
            for name in CJK_FAMILIES {
                if let Some(f) = self.try_family_for_codepoint(&Family::Name(name), codepoint) {
                    return Some(f);
                }
            }
        }

        // Step 5: Last resort — query any sans-serif font
        self.try_family_for_codepoint(&Family::SansSerif, codepoint)
            .or_else(|| self.try_family_for_codepoint(&Family::Serif, codepoint))
    }

    /// 尝试按家族加载字体，并验证字体支持给定码点。
    fn try_family_for_codepoint(&mut self, family: &Family<'_>, codepoint: char) -> Option<FallbackFont> {
        let face_id = self.db.query(&Query {
            families: &[family.clone()],
            weight: Default::default(),
            stretch: Default::default(),
            style: Default::default(),
        })?;

        let font = self.load_font_by_id(face_id)?;

        // Verify the font actually has a glyph for this codepoint
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        if face.glyph_index(codepoint).is_some() {
            Some(font)
        } else {
            None
        }
    }

    /// 加载字体的字形数据。
    fn load_font_by_id(&mut self, face_id: fontdb::ID) -> Option<FallbackFont> {
        let (source, index) = self.db.face_source(face_id)?;

        match source {
            fontdb::Source::File(path) => {
                // Check if already loaded
                if let Some(d) = self.loaded_fonts.get(&path) {
                    return Some(FallbackFont {
                        path: path.clone(),
                        face_index: index,
                        data: d.clone(),
                        is_color: is_color_font(d),
                    });
                }
                let data = std::fs::read(&path).ok()?;
                let is_color = is_color_font(&data);
                self.loaded_fonts.insert(path.clone(), data.clone());
                Some(FallbackFont {
                    path,
                    face_index: index,
                    data,
                    is_color,
                })
            }
            fontdb::Source::Binary(bytes) => {
                let data = bytes.as_ref().as_ref().to_vec();
                let path = PathBuf::from(format!("[memory-{}]", index));
                let is_color = is_color_font(&data);
                Some(FallbackFont {
                    path,
                    face_index: index,
                    data,
                    is_color,
                })
            }
            #[cfg(all(feature = "fs", feature = "memmap"))]
            fontdb::Source::SharedFile(path, _) => {
                let data = std::fs::read(&path).ok()?;
                let is_color = is_color_font(&data);
                Some(FallbackFont {
                    path,
                    face_index: index,
                    data,
                    is_color,
                })
            }
            #[cfg(not(all(feature = "fs", feature = "memmap")))]
            fontdb::Source::SharedFile(_, _) => None,
        }
    }
}

impl Default for FontFallback {
    fn default() -> Self {
        Self::new()
    }
}

// ── Emoji / CJK 检测 ────────────────────────────────────────────────────

/// Returns true if the codepoint is in a Unicode range used for emoji.
fn is_emoji(c: char) -> bool {
    let cp = c as u32;
    in_range(cp, 0x1F600, 0x1F64F)       // 表情符号
    || in_range(cp, 0x1F300, 0x1F5FF)   // 杂项符号和象形文字
    || in_range(cp, 0x1F680, 0x1F6FF)  // 交通和地图
    || in_range(cp, 0x1F900, 0x1F9FF)  // 补充符号和象形文字
    || in_range(cp, 0x1FA00, 0x1FA6F)  // 国际象棋符号
    || in_range(cp, 0x1FA70, 0x1FAFF)  // 符号和象形文字扩展-A
    || in_range(cp, 0x2600, 0x26FF)    // 杂项符号
    || in_range(cp, 0x2700, 0x27BF)    // 装饰符号
    || in_range(cp, 0xFE00, 0xFE0F)    // 变体选择符
    || in_range(cp, 0x1F000, 0x1F02F) // 麻将牌
    || in_range(cp, 0x1F0A0, 0x1F0FF)  // 扑克牌
    || in_range(cp, 0x231A, 0x231B)   // 手表、沙漏
    || in_range(cp, 0x23E9, 0x23F3)    // 各种符号
    || in_range(cp, 0x23F8, 0x23FA)    // 各种符号
    || in_range(cp, 0x25AA, 0x25AB)    // 小方块
    || in_range(cp, 0x25B6, 0x25C0)    // 播放/暂停按钮
    || in_range(cp, 0x25FB, 0x25FE)    // 方块
    || in_range(cp, 0x2614, 0x2615)    // 雨伞、热饮
    || in_range(cp, 0x2648, 0x2653)    // 星座
    || cp == 0x267F                     // 轮椅
    || cp == 0x2693                     // 锚
    || cp == 0x26A1                     // 高压
    || in_range(cp, 0x26AA, 0x26AB)    // 圆圈
    || in_range(cp, 0x26BD, 0x26BE)    // 体育
    || in_range(cp, 0x26C4, 0x26C5)    // 天气
    || in_range(cp, 0x26CE, 0x26CF)    // 蛇夫座
    || cp == 0x26D4                     // 禁止进入
    || in_range(cp, 0x26EA, 0x26EB)    // 教堂/十字架
    || in_range(cp, 0x26F0, 0x26F5)    // 自然
    || cp == 0x26FA                     // 帐篷
    || cp == 0x26FD                     // 加油站
}

fn is_cjk(c: char) -> bool {
    let cp = c as u32;
    in_range(cp, 0x4E00, 0x9FFF)       // CJK 统一表意文字
    || in_range(cp, 0x3400, 0x4DBF)    // CJK 统一表意文字扩展 A
    || in_range(cp, 0x3000, 0x303F)    // CJK 符号和标点
    || in_range(cp, 0x3040, 0x309F)    // 平假名
    || in_range(cp, 0x30A0, 0x30FF)    // 片假名
    || in_range(cp, 0xAC00, 0xD7AF)   // 韩文音节
    || in_range(cp, 0x2E80, 0x2EFF)    // CJK 偏旁补充
    || in_range(cp, 0xF900, 0xFAFF)    // CJK 兼容表意文字
    || in_range(cp, 0x20000, 0x2A6DF)  // CJK 统一表意文字扩展 B+
    || in_range(cp, 0x2F800, 0x2FA1F)  // CJK 兼容表意文字补充
    || in_range(cp, 0x17000, 0x187FF) // 西夏文
    || in_range(cp, 0x18B00, 0x18BFF) // 西夏文部首
}

#[inline]
fn in_range(cp: u32, lo: u32, hi: u32) -> bool {
    cp >= lo && cp <= hi
}

/// 检测字体数据是否包含彩色字形（emoji）。
/// 检查 CBDT/CBLC（彩色位图数据表）或 sbix 表。
fn is_color_font(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    for i in 0..num_tables {
        let offset = 12 + i * 16;
        if offset + 16 > data.len() {
            break;
        }
        let tag = &data[offset..offset + 4];
        if tag == b"CBDT" || tag == b"CBLC" || tag == b"sbix" || tag == b"COLR" {
            return true;
        }
    }
    false
}

/// 要搜索的 Emoji 字体家族（按优先级顺序）。
const EMOJI_FAMILIES: &[&str] = &[
    "Apple Color Emoji",
    "Segoe UI Emoji",
    "Noto Color Emoji",
    "Twitter Color Emoji",
    "EmojiOne Color",
    "Font Awesome 6 Pro",
    "Symbols Nerd Font",
];

/// 要搜索的 CJK 字体家族（按平台排序的优先级顺序）。
#[cfg(target_os = "macos")]
const CJK_FAMILIES: &[&str] = &[
    "PingFang SC",
    "PingFang TC",
    "PingFang HK",
    "Hiragino Sans GB",
    "Yuanti SC",
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
    "Source Han Sans TC",
    "Noto Sans CJK SC",
    "Noto Sans CJK TC",
];
