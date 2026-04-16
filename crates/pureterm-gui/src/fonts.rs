use fontdue::Font;
use std::collections::HashMap;
use std::path::Path;

use crate::sprite::{is_block_element, is_box_drawing, is_braille, is_powerline, draw_block_element, draw_box_drawing, draw_braille, draw_powerline, SpriteMetrics};

// ── Embedded Fonts ─────────────────────────────────────────────────────────

/// Embedded Nerd Font (JetBrains Mono) with full Unicode support including
/// Box-drawing (U+2500-U+257F), Block Elements (U+2580-U+259F), and Powerline symbols.
static EMBEDDED_NERD_FONT: &[u8] = include_bytes!("fonts/JetBrainsMonoNerdFont-Regular.ttf");

/// A glyph in the atlas.
#[derive(Debug, Clone)]
pub struct GlyphInfo {
    pub x: u32,
    pub y: u32,
    pub width: u16,
    pub height: u16,
    pub x_offset: f32,
    pub y_offset: f32,
    pub advance: f32,
    /// Whether this glyph is a ligature (multiple characters combined)
    pub is_ligature: bool,
    /// Original character count if ligature (for advance calculation)
    pub ligature_len: usize,
}

// ── Ligature Engine ──────────────────────────────────────────────────────────

/// Common ligature mappings: (str pair) -> Unicode ligature character
/// These are common programming ligatures supported by fonts like Fira Code, JetBrains Mono
const LIGATURE_PAIRS: &[(&str, char, f32)] = &[
    // Arrows
    ("->", '\u{2192}', 2.0),   // →
    ("<-", '\u{2190}', 2.0),   // ←
    ("=>", '\u{21D2}', 2.0),   // ⇒
    ("<=", '\u{21D0}', 2.0),   // ⇐
    // Comparison
    ("!=", '\u{2260}', 2.0),   // ≠
    (">=", '\u{2265}', 2.0),   // ≥
    ("<=", '\u{2264}', 2.0),   // ≤
    // Logic
    (">>", '\u{226B}', 2.0),   // ≫
    ("<<", '\u{226A}', 2.0),   // ≪
    ("|>", '\u{27D4}', 2.0),   // ⟴
    ("||", '\u{2228}', 2.0),   // ∨
    ("&&", '\u{2227}', 2.0),   // ∧
    // Punctuation / Combined
    ("::", '\u{2A2F}', 2.0),  // ⁄⁄ (double colon)
    (";;", '\u{2E3B}', 2.0),   // ⁄⁄ (double semicolon)
    ("...", '\u{2026}', 3.0),  // …
    // Shell
    ("|-", '\u{2502}', 2.0),  // │ (pipe-dash)
    ("-|", '\u{2502}', 2.0),  // │ (dash-pipe)
];

/// Ligature engine for detecting and resolving character sequences into ligatures.
#[derive(Default)]
pub struct LigatureEngine {
    /// Ligature cache: (char1, char2) -> LigatureInfo
    ligatures: HashMap<(char, char), LigatureInfo>,
}

#[derive(Clone)]
pub struct LigatureInfo {
    ch: char,
    width_factor: f32,
}

impl LigatureEngine {
    pub fn new() -> Self {
        let mut engine = Self::default();
        engine.build_ligature_table();
        engine
    }

    /// Build the ligature lookup table from LIGATURE_PAIRS
    fn build_ligature_table(&mut self) {
        for (pair, ch, width_factor) in LIGATURE_PAIRS {
            let mut chars = pair.chars();
            if let (Some(c1), Some(c2)) = (chars.next(), chars.next()) {
                // Only handle 2-char ligatures for now (simpler advance calculation)
                self.ligatures.insert((c1, c2), LigatureInfo {
                    ch: *ch,
                    width_factor: *width_factor,
                });
            }
        }
    }

    /// Check if two consecutive characters form a ligature.
    /// Returns the ligature info if found, None otherwise.
    pub fn get_ligature(&self, c1: char, c2: char) -> Option<LigatureInfo> {
        self.ligatures.get(&(c1, c2)).cloned()
    }
}

/// Skyline segment for rectangle bin packing.
#[derive(Debug, Clone)]
struct SkylineSeg {
    x: u32,
    y: u32,
    width: u32,
}

/// Font atlas that caches rendered glyphs.
/// Font face indices for different styles.
pub mod face {
    pub const REGULAR: usize = 0;
    pub const BOLD: usize = 1;
    pub const ITALIC: usize = 2;
    pub const BOLD_ITALIC: usize = 3;
}

pub struct GlyphAtlas {
    /// Font faces: [regular, bold, italic, bold_italic]
    fonts: [Option<Font>; 4],
    font_size: f32,
    ascent: f32,
    cache: HashMap<(usize, char), GlyphInfo>,
    atlas_width: u32,
    atlas_height: u32,
    atlas_data: Vec<u8>,
    /// Skyline segments for bin packing.
    skyline: Vec<SkylineSeg>,
    /// Ligature engine for character sequence detection
    ligature_engine: LigatureEngine,
    /// Ligature-enabled fonts (loaded font supports ligatures)
    ligature_enabled: bool,
    /// Maximum atlas height (for auto-grow).
    max_atlas_height: u32,
}

impl GlyphAtlas {
    pub fn new(font_size: f32) -> Self {
        let w = 2048u32;
        let h = 2048u32;
        let mut atlas_data = vec![0; (w * h) as usize];
        // Reserve pixel (0,0) as white for background quads
        atlas_data[0] = 255;
        Self {
            fonts: [None, None, None, None],
            font_size,
            ascent: 0.0,
            cache: HashMap::new(),
            atlas_width: w,
            atlas_height: h,
            atlas_data,
            skyline: vec![SkylineSeg { x: 1, y: 0, width: w - 1 }], // start after reserved pixel
            ligature_engine: LigatureEngine::new(),
            ligature_enabled: true,
            max_atlas_height: 8192,
        }
    }

    /// Load a font from bytes into a specific face slot.
    pub fn add_font_to_face(&mut self, font_data: &[u8], face_idx: usize) -> Result<(), String> {
        let font = Font::from_bytes(font_data, fontdue::FontSettings::default())
            .map_err(|e| format!("Failed to load font: {:?}", e))?;

        // Compute ascent from the regular face
        if face_idx == face::REGULAR && self.ascent == 0.0 {
            if let Some(line_metrics) = font.horizontal_line_metrics(self.font_size) {
                self.ascent = line_metrics.ascent;
            }
            // Test rasterization of Block Elements to verify font support
            let test_chars = ['A', '\u{2588}', '\u{2590}', '\u{259B}'];
            for &c in &test_chars {
                let (metrics, _) = font.rasterize(c, self.font_size);
                log::debug!("Font test glyph '{}' (U+{:04X}): {}x{}", c, c as u32, metrics.width, metrics.height);
            }
        }

        if face_idx < 4 {
            self.fonts[face_idx] = Some(font);
        }
        Ok(())
    }

    /// Load a font from bytes into the first available face slot.
    pub fn add_font(&mut self, font_data: &[u8]) -> Result<(), String> {
        let face_idx = self.fonts.iter().position(|f| f.is_none()).unwrap_or(0);
        self.add_font_to_face(font_data, face_idx)
    }

    /// Load a bold font face from bytes.
    pub fn add_bold_font(&mut self, font_data: &[u8]) -> Result<(), String> {
        self.add_font_to_face(font_data, face::BOLD)
    }

    /// Load an italic font face from bytes.
    pub fn add_italic_font(&mut self, font_data: &[u8]) -> Result<(), String> {
        self.add_font_to_face(font_data, face::ITALIC)
    }

    /// Load a bold-italic font face from bytes.
    pub fn add_bold_italic_font(&mut self, font_data: &[u8]) -> Result<(), String> {
        self.add_font_to_face(font_data, face::BOLD_ITALIC)
    }

    /// Get the appropriate font face index for the given SGR attributes.
    pub fn face_index_for_attrs(&self, bold: bool, italic: bool) -> usize {
        match (bold, italic) {
            (false, false) => face::REGULAR,
            (true, false) => {
                if self.fonts[face::BOLD].is_some() {
                    face::BOLD
                } else {
                    face::REGULAR // fallback to regular (synthetic bold via SGR)
                }
            }
            (false, true) => {
                if self.fonts[face::ITALIC].is_some() {
                    face::ITALIC
                } else {
                    face::REGULAR
                }
            }
            (true, true) => {
                if self.fonts[face::BOLD_ITALIC].is_some() {
                    face::BOLD_ITALIC
                } else if self.fonts[face::BOLD].is_some() {
                    face::BOLD
                } else if self.fonts[face::ITALIC].is_some() {
                    face::ITALIC
                } else {
                    face::REGULAR
                }
            }
        }
    }

    /// Check if a font face is loaded.
    pub fn has_face(&self, face_idx: usize) -> bool {
        self.fonts.get(face_idx).map_or(false, |f| f.is_some())
    }

    /// Load a font from a file path.
    pub fn load_font(&mut self, path: &Path) -> Result<(), String> {
        let data = std::fs::read(path)
            .map_err(|e| format!("Failed to read font file: {}", e))?;
        self.add_font(&data)
    }

    /// Load the default monospace font.
    /// If a custom font path is provided and the file exists, it will be loaded instead.
    /// Otherwise, falls back to the embedded Nerd Font, then system fonts.

    /// Allocate a rectangle in the atlas using skyline bin packing.
    /// Returns (x, y) of the allocated region, or None if no space.
    fn allocate_rect(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        let padding = 1u32;
        let rw = width + padding;
        let rh = height + padding;

        // Find the best skyline segment (lowest resulting y)
        let mut best_idx = 0;
        let mut best_y = u32::MAX;
        let mut best_x = 0;

        for (i, seg) in self.skyline.iter().enumerate() {
            if seg.x + rw > self.atlas_width {
                continue;
            }
            // Compute the y-level if we place here
            let mut y = seg.y;
            let mut remaining_w = rw;
            let mut j = i;
            let mut x = seg.x;
            while remaining_w > 0 && j < self.skyline.len() {
                let s = &self.skyline[j];
                if s.x > x {
                    // Gap between segments - not contiguous
                    break;
                }
                y = y.max(s.y);
                if y + rh > self.atlas_height {
                    break;
                }
                let overlap = (s.x + s.width).min(seg.x + rw) - x;
                remaining_w = remaining_w.saturating_sub(overlap);
                x = s.x + s.width;
                j += 1;
            }
            if remaining_w == 0 && y + rh <= self.atlas_height && y < best_y {
                best_y = y;
                best_x = seg.x;
                best_idx = i;
            }
        }

        if best_y == u32::MAX {
            // Try to grow atlas vertically
            let new_height = (self.atlas_height * 2).min(self.max_atlas_height);
            if new_height > self.atlas_height {
                self.atlas_data.resize((self.atlas_width * new_height) as usize, 0);
                self.atlas_height = new_height;
                // Retry allocation
                return self.allocate_rect(width, height);
            }
            return None;
        }

        // Update skyline: add new segment at (best_x, best_y + rh, rw)
        // Merge with adjacent segments
        let new_seg = SkylineSeg { x: best_x, y: best_y + rh, width: rw };

        // Find where to insert/replace
        let insert_pos = best_idx;
        let x_end = best_x + rw;

        // Remove segments covered by the new allocation
        let i = best_idx;
        while i < self.skyline.len() {
            let seg = &self.skyline[i];
            if seg.x >= x_end {
                break;
            }
            let seg_end = seg.x + seg.width;
            if seg_end <= x_end {
                // Fully covered - remove
                self.skyline.remove(i);
                continue;
            }
            // Partially covered - trim left
            self.skyline[i].x = x_end;
            self.skyline[i].width = seg_end - x_end;
            break;
        }

        // Insert new segment
        self.skyline.insert(insert_pos, new_seg);

        // Merge adjacent segments at the same y level
        let mut i = 0;
        while i + 1 < self.skyline.len() {
            let curr_y = self.skyline[i].y;
            let next_y = self.skyline[i + 1].y;
            let curr_end = self.skyline[i].x + self.skyline[i].width;
            let next_start = self.skyline[i + 1].x;
            if curr_y == next_y && curr_end == next_start {
                self.skyline[i].width += self.skyline[i + 1].width;
                self.skyline.remove(i + 1);
            } else {
                i += 1;
            }
        }

        Some((best_x, best_y))
    }
    pub fn load_default_font(&mut self, custom_font_path: Option<&str>) -> Result<(), String> {
        // 1. If custom font path is provided, try to load it first
        if let Some(path) = custom_font_path {
            let font_path = Path::new(path);
            if font_path.exists() {
                match self.load_font(font_path) {
                    Ok(()) => {
                        log::info!("Loaded custom font: {}", path);
                        return Ok(());
                    }
                    Err(e) => {
                        log::warn!("Failed to load custom font {}: {}", path, e);
                        // Continue to fallback options
                    }
                }
            } else {
                log::warn!("Custom font file not found: {}", path);
            }
        }

        // 2. Try to load the embedded Nerd Font
        match self.add_font(EMBEDDED_NERD_FONT) {
            Ok(()) => {
                log::info!("Loaded embedded JetBrains Mono Nerd Font");
            }
            Err(e) => {
                log::warn!("Failed to load embedded font: {}", e);
                // Fall back to system fonts
                self.load_system_font()?;
            }
        }
        Ok(())
    }

    /// Load system font as fallback.
    fn load_system_font(&mut self) -> Result<(), String> {
        // First try platform-specific font discovery
        if let Some(path) = self.discover_system_font() {
            log::info!("Discovered system font: {}", path);
            return self.load_font(Path::new(&path));
        }

        // Fallback: try well-known paths
        #[cfg(target_os = "macos")]
        {
            let paths = [
                "/System/Library/Fonts/Menlo.ttc",
                "/System/Library/Fonts/SFNSMono.ttf",
                "/System/Library/Fonts/Monaco.ttf",
                "/Library/Fonts/Menlo.ttc",
            ];
            for path in &paths {
                if Path::new(path).exists() {
                    match self.load_font(Path::new(path)) {
                        Ok(()) => {
                            log::info!("Loaded system font: {}", path);
                            return Ok(());
                        }
                        Err(e) => {
                            log::warn!("Failed to load font {}: {}", path, e);
                        }
                    }
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            let paths = [
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
                "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
                "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
            ];
            for path in &paths {
                if Path::new(path).exists() {
                    match self.load_font(Path::new(path)) {
                        Ok(()) => {
                            log::info!("Loaded system font: {}", path);
                            return Ok(());
                        }
                        Err(e) => {
                            log::warn!("Failed to load font {}: {}", path, e);
                        }
                    }
                }
            }
        }

        Err("No suitable system font found. Please install a monospace font.".to_string())
    }

    /// Discover a monospace system font using platform APIs.
    fn discover_system_font(&self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            // Use macOS system_profiler or python to find monospace fonts
            // Try using Python (available on all macOS) to call CoreText
            let script = r#"
import subprocess, json
try:
    result = subprocess.run(
        ["system_profiler", "SPFontsDataType", "-json"],
        capture_output=True, text=True, timeout=10
    )
    data = json.loads(result.stdout)
    for font in data.get("SPFontsDataType", []):
        family = font.get("family", "")
        path = font.get("path", "")
        if "mono" in family.lower() and path.endswith((".ttf", ".otf", ".ttc")):
            print(path)
            break
except:
    pass
"#;
            if let Ok(output) = std::process::Command::new("/usr/bin/python3")
                .arg("-c")
                .arg(script)
                .output()
            {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() && Path::new(&path).exists() {
                    return Some(path);
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Use fc-list to find monospace fonts
            if let Ok(output) = std::process::Command::new("fc-list")
                .args([":spacing=mono", "file"])
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let path = line.trim().trim_end_matches(':');
                    if !path.is_empty() && Path::new(path).exists() {
                        return Some(path.to_string());
                    }
                }
            }
        }

        None
    }

    /// Discover and load bold/italic font variants for the current font family.
    pub fn load_font_variants(&mut self, family_name: Option<&str>) {
        let family = family_name.map(|s| s.to_string()).or_else(|| {
            // Try to detect family from the loaded font
            None
        });

        let search_patterns: Vec<(&str, usize)> = vec![
            ("Bold", face::BOLD),
            ("Italic", face::ITALIC),
            ("BoldItalic", face::BOLD_ITALIC),
        ];

        for (pattern, face_idx) in search_patterns {
            if self.has_face(face_idx) {
                continue;
            }
            if let Some(path) = self.discover_variant(&family, pattern) {
                match std::fs::read(&path) {
                    Ok(data) => {
                        if let Err(e) = self.add_font_to_face(&data, face_idx) {
                            log::debug!("Failed to load variant font {}: {}", path, e);
                        } else {
                            log::info!("Loaded {} font variant: {}", pattern, path);
                        }
                    }
                    Err(e) => {
                        log::debug!("Failed to read variant font {}: {}", path, e);
                    }
                }
            }
        }
    }

    /// Discover a font variant by searching for it on the system.
    fn discover_variant(&self, family: &Option<String>, style: &str) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            let family_name = family.as_deref().unwrap_or("JetBrainsMono");
            let style_lower = style.to_lowercase();
            // Search in common font directories
            let dirs = [
                "/System/Library/Fonts",
                "/Library/Fonts",
                &format!("/Users/{}/Library/Fonts", std::env::var("USER").unwrap_or_default()),
            ];
            for dir in &dirs {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                        if name.contains(&family_name.to_lowercase())
                            && name.contains(&style_lower)
                            && (name.ends_with(".ttf") || name.ends_with(".otf") || name.ends_with(".ttc"))
                        {
                            return Some(path.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            let style_lower = style.to_lowercase();
            let style_arg = match style {
                "Bold" => ":bold",
                "Italic" => ":italic",
                "BoldItalic" => ":bold:italic",
                _ => "",
            };
            if let Ok(output) = std::process::Command::new("fc-list")
                .args([style_arg, "file"])
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let path = line.trim().trim_end_matches(':');
                    if !path.is_empty() && Path::new(path).exists() {
                        return Some(path.to_string());
                    }
                }
            }
        }

        // Fallback: try embedded font for JetBrains Mono variants
        if style == "Bold" {
            // Try to load bold variant from the fonts directory
            let bold_path = "crates/pureterm-gui/src/fonts/JetBrainsMonoNerdFont-Bold.ttf";
            if Path::new(bold_path).exists() {
                return Some(bold_path.to_string());
            }
        }

        None
    }

    /// Load a user-configured font from file path.
    /// This clears existing fonts and loads the new font.
    pub fn load_font_from_path(&mut self, path: &str) -> Result<(), String> {
        let font_path = Path::new(path);
        if !font_path.exists() {
            return Err(format!("Font file not found: {}", path));
        }

        // Clear existing fonts
        self.fonts = [None, None, None, None];
        self.cache.clear();
        // Reset skyline
        self.atlas_data = vec![0; (self.atlas_width * self.atlas_height) as usize];
        self.atlas_data[0] = 255;
        self.skyline = vec![SkylineSeg { x: 1, y: 0, width: self.atlas_width - 1 }];

        self.load_font(font_path)?;
        log::info!("Loaded user font: {}", path);
        Ok(())
    }

    /// Get or rasterize a glyph.
    pub fn get_glyph(&mut self, font_index: usize, c: char) -> Option<&GlyphInfo> {
        if self.fonts[face::REGULAR].is_none() {
            return None;
        }

        let key = (font_index, c);
        if self.cache.contains_key(&key) {
            return self.cache.get(&key);
        }

        // Use sprite drawing for special terminal characters
        if is_block_element(c) {
            eprintln!("SPRITE PATH: block_element '{}' (U+{:04X})", c, c as u32);
            log::debug!("Sprite glyph: block_element '{}' (U+{:04X})", c, c as u32);
            return self.get_sprite_glyph(font_index, c);
        }
        if is_box_drawing(c) {
            eprintln!("SPRITE PATH: box_drawing '{}' (U+{:04X})", c, c as u32);
            log::debug!("Sprite glyph: box_drawing '{}' (U+{:04X})", c, c as u32);
            return self.get_box_drawing_glyph(font_index, c);
        }
        if is_braille(c) {
            eprintln!("SPRITE PATH: braille '{}' (U+{:04X})", c, c as u32);
            log::debug!("Sprite glyph: braille '{}' (U+{:04X})", c, c as u32);
            return self.get_braille_glyph(font_index, c);
        }
        if is_powerline(c) {
            eprintln!("SPRITE PATH: powerline '{}' (U+{:04X})", c, c as u32);
            log::debug!("Sprite glyph: powerline '{}' (U+{:04X})", c, c as u32);
            return self.get_powerline_glyph(font_index, c);
        }

        let font = &self.fonts.get(font_index)?.as_ref()?;
        let (metrics, bitmap) = font.rasterize(c, self.font_size);

        let width = metrics.width as u16;
        let height = metrics.height as u16;

        // Skip zero-dimension glyphs (font doesn't support this character)
        if width == 0 || height == 0 {
            log::debug!("Glyph for '{}' (U+{:04X}) has zero dimensions ({}x{}), skipping",
                c, c as u32, width, height);
            return None;
        }

        // Debug special characters
        let code = c as u32;
        if (code >= 0x2500 && code <= 0x257F) || (code >= 0x2580 && code <= 0x259F) || (code >= 0x2800 && code <= 0x28FF) {
            eprintln!("FONT PATH: '{}' (U+{:04X}) rasterized to {}x{}", c, code, width, height);
        }

        // Allocate space in the atlas using skyline bin packing
        let (ax, ay) = match self.allocate_rect(width as u32, height as u32) {
            Some(pos) => pos,
            None => {
                log::warn!("Glyph atlas full, cannot cache more glyphs");
                return None;
            }
        };

        // Write the glyph to the atlas
        let start_x = ax as usize;
        let start_y = ay as usize;
        for (gy, row) in bitmap.chunks(width as usize).enumerate() {
            for (gx, &val) in row.iter().enumerate() {
                let px = start_x + gx;
                let py = start_y + gy;
                if px < self.atlas_width as usize && py < self.atlas_height as usize {
                    self.atlas_data[py * self.atlas_width as usize + px] = val;
                }
            }
        }

        let info = GlyphInfo {
            x: ax,
            y: ay,
            width,
            height,
            x_offset: metrics.xmin as f32,
            y_offset: metrics.ymin as f32,
            advance: metrics.advance_width,
            is_ligature: false,
            ligature_len: 1,
        };

        self.cache.insert(key, info);
        self.cache.get(&key)
    }

    /// Get or draw a sprite glyph for Block Elements.
    fn get_sprite_glyph(&mut self, font_index: usize, c: char) -> Option<&GlyphInfo> {
        let key = (font_index, c);

        // Verify font exists (needed for cache key validation)
        let _ = self.fonts.get(font_index)?.as_ref()?;

        // Compute cell dimensions (ceil to avoid sub-pixel gaps)
        let (cell_w_f32, cell_h_f32) = self.cell_size();
        let cell_w = cell_w_f32 as u32;
        let cell_h = cell_h_f32.ceil() as u32;

        // Allocate space using skyline bin packing
        let (mut ax, mut ay) = match self.allocate_rect(cell_w, cell_h) {
            Some(pos) => pos,
            None => {
                log::warn!("Glyph atlas full, cannot cache sprite glyphs");
                return None;
            }
        };

        let metrics = SpriteMetrics::new(cell_w, cell_h, self.font_size);

        // Draw the block element using sprite drawing
        let glyph_info = draw_block_element(
            &mut self.atlas_data,
            self.atlas_width,
            &metrics,
            c,
            &mut ax,
            &mut ay,
        );

        // Manually advance ax after draw_block_element returns.
        // The cursor advancement inside draw_block_element operates on local copies
        // passed via &mut ax, but we need to also advance our local ax for the next
        // allocation to work correctly with the skyline.
        ax += cell_w + 1;

        if let Some(mut info) = glyph_info {
            // Fix y_offset so the sprite glyph exactly covers the cell area.
            info.y_offset = self.ascent - cell_h as f32;
            self.cache.insert(key, info);
            return self.cache.get(&key);
        }

        None
    }

    /// Get or draw a sprite glyph for Box Drawing characters.
    fn get_box_drawing_glyph(&mut self, font_index: usize, c: char) -> Option<&GlyphInfo> {
        let key = (font_index, c);

        // Verify font exists (needed for cache key validation)
        let _ = self.fonts.get(font_index)?.as_ref()?;

        // Compute cell dimensions (ceil to avoid sub-pixel gaps)
        let (cell_w_f32, cell_h_f32) = self.cell_size();
        let cell_w = cell_w_f32 as u32;
        let cell_h = cell_h_f32.ceil() as u32;

        // Allocate space using skyline bin packing
        let (mut ax, mut ay) = match self.allocate_rect(cell_w, cell_h) {
            Some(pos) => pos,
            None => {
                log::warn!("Glyph atlas full, cannot cache box drawing glyphs");
                return None;
            }
        };

        let metrics = SpriteMetrics::new(cell_w, cell_h, self.font_size);

        // Draw the box drawing character using sprite drawing
        let glyph_info = draw_box_drawing(
            &mut self.atlas_data,
            self.atlas_width,
            &metrics,
            c,
            &mut ax,
            &mut ay,
        );

        // Manually advance ax after draw_box_drawing returns (same reason as above)
        ax += cell_w + 1;

        if let Some(mut info) = glyph_info {
            info.y_offset = self.ascent - cell_h as f32;
            self.cache.insert(key, info);
            return self.cache.get(&key);
        }

        None
    }

    /// Get or draw a sprite glyph for Braille characters.
    fn get_braille_glyph(&mut self, font_index: usize, c: char) -> Option<&GlyphInfo> {
        let key = (font_index, c);

        // Verify font exists (needed for cache key validation)
        let _ = self.fonts.get(font_index)?.as_ref()?;

        // Compute cell dimensions (ceil to avoid sub-pixel gaps)
        let (cell_w_f32, cell_h_f32) = self.cell_size();
        let cell_w = cell_w_f32 as u32;
        let cell_h = cell_h_f32.ceil() as u32;

        // Allocate space using skyline bin packing
        let (mut ax, mut ay) = match self.allocate_rect(cell_w, cell_h) {
            Some(pos) => pos,
            None => {
                log::warn!("Glyph atlas full, cannot cache braille glyphs");
                return None;
            }
        };

        let metrics = SpriteMetrics::new(cell_w, cell_h, self.font_size);

        // Draw the braille character using sprite drawing
        let glyph_info = draw_braille(
            &mut self.atlas_data,
            self.atlas_width,
            self.atlas_height,
            &metrics,
            c,
            &mut ax,
            &mut ay,
        );

        // Manually advance ax after draw_braille returns (same reason as above)
        ax += cell_w + 1;

        if let Some(mut info) = glyph_info {
            info.y_offset = self.ascent - cell_h as f32;
            self.cache.insert(key, info);
            return self.cache.get(&key);
        }

        None
    }

    /// Get or draw a sprite glyph for Powerline symbols.
    fn get_powerline_glyph(&mut self, font_index: usize, c: char) -> Option<&GlyphInfo> {
        let key = (font_index, c);

        // Verify font exists (needed for cache key validation)
        let _ = self.fonts.get(font_index)?.as_ref()?;

        // Compute cell dimensions (ceil to avoid sub-pixel gaps)
        let (cell_w_f32, cell_h_f32) = self.cell_size();
        let cell_w = cell_w_f32 as u32;
        let cell_h = cell_h_f32.ceil() as u32;

        // Allocate space using skyline bin packing
        let (mut ax, mut ay) = match self.allocate_rect(cell_w, cell_h) {
            Some(pos) => pos,
            None => {
                log::warn!("Glyph atlas full, cannot cache powerline glyphs");
                return None;
            }
        };

        let metrics = SpriteMetrics::new(cell_w, cell_h, self.font_size);

        // Draw the powerline character using sprite drawing
        let glyph_info = draw_powerline(
            &mut self.atlas_data,
            self.atlas_width,
            self.atlas_height,
            &metrics,
            c,
            &mut ax,
            &mut ay,
        );

        // Manually advance ax after draw_powerline returns (same reason as above)
        ax += cell_w + 1;

        if let Some(mut info) = glyph_info {
            info.y_offset = self.ascent - cell_h as f32;
            self.cache.insert(key, info);
            return self.cache.get(&key);
        }

        None
    }

    /// Check if two consecutive characters form a ligature.
    /// Returns the ligature character and width factor if found.
    pub fn get_ligature(&self, c1: char, c2: char) -> Option<(char, f32)> {
        if !self.ligature_enabled {
            return None;
        }
        self.ligature_engine
            .get_ligature(c1, c2)
            .map(|info| (info.ch, info.width_factor))
    }

    /// Get or rasterize a ligature glyph.
    /// This handles cases where two characters combine into one glyph (e.g., -> becomes →).
    pub fn get_ligature_glyph(&mut self, font_index: usize, lig_char: char, width_factor: f32) -> Option<&GlyphInfo> {
        if self.fonts.is_empty() {
            return None;
        }

        let key = (font_index, lig_char);
        if self.cache.contains_key(&key) {
            return self.cache.get(&key);
        }

        let font = &self.fonts.get(font_index)?.as_ref()?;
        let (metrics, bitmap) = font.rasterize(lig_char, self.font_size);

        let width = metrics.width as u16;
        let height = metrics.height as u16;

        // Skip zero-dimension glyphs (font doesn't support this character)
        if width == 0 || height == 0 {
            return None;
        }

        // Allocate space using skyline bin packing
        let (ax, ay) = match self.allocate_rect(width as u32, height as u32) {
            Some(pos) => pos,
            None => {
                log::warn!("Glyph atlas full, cannot cache more glyphs");
                return None;
            }
        };

        // Write the glyph to the atlas
        let start_x = ax as usize;
        let start_y = ay as usize;
        for (gy, row) in bitmap.chunks(width as usize).enumerate() {
            for (gx, &val) in row.iter().enumerate() {
                let px = start_x + gx;
                let py = start_y + gy;
                if px < self.atlas_width as usize && py < self.atlas_height as usize {
                    self.atlas_data[py * self.atlas_width as usize + px] = val;
                }
            }
        }

        let info = GlyphInfo {
            x: ax,
            y: ay,
            width,
            height,
            x_offset: metrics.xmin as f32,
            y_offset: metrics.ymin as f32,
            advance: metrics.advance_width * width_factor,
            is_ligature: true,
            ligature_len: 2,
        };

        self.cache.insert(key, info);
        self.cache.get(&key)
    }

    /// Get the font size.
    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// Get the font ascent (distance from cell top to baseline).
    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    /// Set the font size (clears cache).
    pub fn set_font_size(&mut self, size: f32) {
        if (self.font_size - size).abs() > f32::EPSILON {
            self.font_size = size;
            self.cache.clear();
            self.atlas_data = vec![0; (self.atlas_width * self.atlas_height) as usize];
            self.atlas_data[0] = 255; // Re-reserve background pixel
            self.skyline = vec![SkylineSeg { x: 1, y: 0, width: self.atlas_width - 1 }];
        }
    }

    /// Enable or disable ligatures.
    pub fn set_ligatures_enabled(&mut self, enabled: bool) {
        self.ligature_enabled = enabled;
    }

    /// Check if ligatures are enabled.
    pub fn ligatures_enabled(&self) -> bool {
        self.ligature_enabled
    }

    /// Get the cell dimensions for the current font size.
    pub fn cell_size(&self) -> (f32, f32) {
        if let Some(font) = self.fonts[face::REGULAR].as_ref() {
            let (metrics, _) = font.rasterize('M', self.font_size);
            let width = metrics.advance_width;
            let height = self.font_size * 1.2;
            (width, height)
        } else {
            (8.0, 16.0)
        }
    }

    /// Get the line height in pixels.
    pub fn line_height(&self) -> u32 {
        (self.font_size * 1.2) as u32
    }

    /// Get decoration metrics derived from font data.
    /// Returns (underline_position, underline_thickness, strikethrough_position, strikethrough_thickness)
    /// where positions are relative to the baseline (positive = below).
    pub fn decoration_metrics(&self) -> (f32, f32, f32, f32) {
        if let Some(font) = self.fonts[face::REGULAR].as_ref() {
            if let Some(line_metrics) = font.horizontal_line_metrics(self.font_size) {
                let descent = -line_metrics.descent; // positive value going down
                // Underline: typically at descent * 0.5, thickness proportional to font size
                let underline_pos = descent * 0.5;
                let underline_thickness = (self.font_size / 14.0).max(1.0);
                // Strikethrough: at approximately ascender center
                let strike_pos = line_metrics.ascent * 0.3;
                let strike_thickness = underline_thickness;
                (underline_pos, underline_thickness, strike_pos, strike_thickness)
            } else {
                // Fallback based on font size
                let t = (self.font_size / 14.0).max(1.0);
                (self.font_size * 0.15, t, self.font_size * 0.3, t)
            }
        } else {
            (2.0, 1.0, 5.0, 1.0)
        }
    }

    /// Get the box drawing line thickness derived from font metrics.
    pub fn box_thickness(&self) -> f32 {
        let (_, cell_h) = self.cell_size();
        (cell_h / 10.0).max(1.0)
    }

    /// Get the atlas texture data.
    pub fn atlas_data(&self) -> &[u8] {
        &self.atlas_data
    }

    /// Get atlas dimensions.
    pub fn atlas_dimensions(&self) -> (u32, u32) {
        (self.atlas_width, self.atlas_height)
    }

    /// Get the number of loaded fonts.
    pub fn font_count(&self) -> usize {
        self.fonts.iter().filter(|f| f.is_some()).count()
    }
}
