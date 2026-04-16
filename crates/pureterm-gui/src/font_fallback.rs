//! Font fallback chain resolver using fontdb.
//!
//! 5-step resolution:
//! 1. Primary font (user-configured) — handled by GlyphAtlas
//! 2. Family fallback (system font matching codepoint)
//! 3. Emoji fallback (Apple Color Emoji, Noto Color Emoji, etc.)
//! 4. CJK fallback (Noto Sans CJK, PingFang, etc.)
//! 5. Last resort (any available font)

use fontdb::{Database, Family, Query};
use std::path::PathBuf;

/// A font identified for fallback, with its raw data loaded.
#[derive(Debug, Clone)]
pub struct FallbackFont {
    /// Full path to the font file.
    pub path: PathBuf,
    /// Face index within the font file (for TTC/OTC collections).
    pub face_index: u32,
    /// Raw font data bytes.
    pub data: Vec<u8>,
    /// Whether this font has color glyphs (emoji).
    pub is_color: bool,
}

/// Font fallback resolver using fontdb.
pub struct FontFallback {
    db: Database,
    /// Loaded font data by font path (for deduplication).
    loaded_fonts: std::collections::HashMap<PathBuf, Vec<u8>>,
}

impl FontFallback {
    /// Create a new fallback resolver and scan system fonts.
    pub fn new() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        Self {
            db,
            loaded_fonts: std::collections::HashMap::new(),
        }
    }

    /// Find a fallback font for the given codepoint.
    /// Returns a fully loaded font with its data.
    pub fn find(&mut self, codepoint: char) -> Option<FallbackFont> {
        // Step 2: Try family fallback (system monospace)
        if let Some(f) = self.try_family(&Family::Monospace) {
            return Some(f);
        }

        // Step 3: Emoji fallback
        if is_emoji(codepoint) {
            for name in EMOJI_FAMILIES {
                if let Some(f) = self.try_family(&Family::Name(name)) {
                    return Some(f);
                }
            }
        }

        // Step 4: CJK fallback
        if is_cjk(codepoint) {
            #[cfg(target_os = "macos")]
            {
                if let Some(f) = self.try_family(&Family::Name("Apple Color Emoji")) {
                    return Some(f);
                }
            }
            for name in CJK_FAMILIES {
                if let Some(f) = self.try_family(&Family::Name(name)) {
                    return Some(f);
                }
            }
        }

        // Step 5: Last resort — query any sans-serif font
        self.try_family(&Family::SansSerif)
            .or_else(|| self.try_family(&Family::Serif))
    }

    /// Try to load a font by family.
    fn try_family(&mut self, family: &Family<'_>) -> Option<FallbackFont> {
        let face_id = self.db.query(&Query {
            families: &[family.clone()],
            weight: Default::default(),
            stretch: Default::default(),
            style: Default::default(),
        })?;

        self.load_font_by_id(face_id)
    }

    /// Load font data for a face ID.
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

// ── Emoji / CJK detection ────────────────────────────────────────────────────

/// Returns true if the codepoint is in a Unicode range used for emoji.
fn is_emoji(c: char) -> bool {
    let cp = c as u32;
    in_range(cp, 0x1F600, 0x1F64F)       // Emoticons
    || in_range(cp, 0x1F300, 0x1F5FF)   // Misc Symbols and Pictographs
    || in_range(cp, 0x1F680, 0x1F6FF)  // Transport and Map
    || in_range(cp, 0x1F900, 0x1F9FF)  // Supplemental Symbols and Pictographs
    || in_range(cp, 0x1FA00, 0x1FA6F)  // Chess Symbols
    || in_range(cp, 0x1FA70, 0x1FAFF)  // Symbols and Pictographs Extended-A
    || in_range(cp, 0x2600, 0x26FF)    // Misc symbols
    || in_range(cp, 0x2700, 0x27BF)    // Dingbats
    || in_range(cp, 0xFE00, 0xFE0F)    // Variation Selectors
    || in_range(cp, 0x1F000, 0x1F02F) // Mahjong Tiles
    || in_range(cp, 0x1F0A0, 0x1F0FF)  // Playing Cards
    || in_range(cp, 0x231A, 0x231B)   // Watch, Hourglass
    || in_range(cp, 0x23E9, 0x23F3)    // Various symbols
    || in_range(cp, 0x23F8, 0x23FA)    // Various symbols
    || in_range(cp, 0x25AA, 0x25AB)    // Small squares
    || in_range(cp, 0x25B6, 0x25C0)    // Play/pause buttons
    || in_range(cp, 0x25FB, 0x25FE)    // Squares
    || in_range(cp, 0x2614, 0x2615)    // Umbrella, hot beverage
    || in_range(cp, 0x2648, 0x2653)    // Zodiac
    || cp == 0x267F                     // Wheelchair
    || cp == 0x2693                     // Anchor
    || cp == 0x26A1                     // High voltage
    || in_range(cp, 0x26AA, 0x26AB)    // Circles
    || in_range(cp, 0x26BD, 0x26BE)    // Sports
    || in_range(cp, 0x26C4, 0x26C5)    // Weather
    || in_range(cp, 0x26CE, 0x26CF)    // Ophiuchus
    || cp == 0x26D4                     // No entry
    || in_range(cp, 0x26EA, 0x26EB)    // Church/Cross
    || in_range(cp, 0x26F0, 0x26F5)    // Nature
    || cp == 0x26FA                     // Tent
    || cp == 0x26FD                     // Fuel pump
}

fn is_cjk(c: char) -> bool {
    let cp = c as u32;
    in_range(cp, 0x4E00, 0x9FFF)       // CJK Unified Ideographs
    || in_range(cp, 0x3400, 0x4DBF)    // CJK Unified Ideographs Extension A
    || in_range(cp, 0x3000, 0x303F)    // CJK Symbols and Punctuation
    || in_range(cp, 0x3040, 0x309F)    // Hiragana
    || in_range(cp, 0x30A0, 0x30FF)    // Katakana
    || in_range(cp, 0xAC00, 0xD7AF)   // Hangul Syllables
    || in_range(cp, 0x2E80, 0x2EFF)    // CJK Radicals Supplement
    || in_range(cp, 0xF900, 0xFAFF)    // CJK Compatibility Ideographs
    || in_range(cp, 0x20000, 0x2A6DF)  // CJK Unified Ideographs Extension B+
    || in_range(cp, 0x2F800, 0x2FA1F)  // CJK Compatibility Ideographs Supplement
    || in_range(cp, 0x17000, 0x187FF) // Tangut
    || in_range(cp, 0x18B00, 0x18BFF) // Tangut Components
}

#[inline]
fn in_range(cp: u32, lo: u32, hi: u32) -> bool {
    cp >= lo && cp <= hi
}

/// Detect if font data contains color glyphs (emoji).
/// Checks for CBDT/CBLC (Color Bitmap Data Tables) or sbix table.
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

/// Emoji font families to search (in priority order).
const EMOJI_FAMILIES: &[&str] = &[
    "Apple Color Emoji",
    "Segoe UI Emoji",
    "Noto Color Emoji",
    "Twitter Color Emoji",
    "EmojiOne Color",
    "Font Awesome 6 Pro",
    "Symbols Nerd Font",
];

/// CJK font families to search (in priority order per platform).
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
