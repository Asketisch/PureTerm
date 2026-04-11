use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;
use std::collections::HashMap;
use std::path::Path;

/// A glyph in the atlas.
#[derive(Debug, Clone)]
pub struct GlyphInfo {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub x_offset: f32,
    pub y_offset: f32,
    pub advance: f32,
}

/// Font atlas that caches rendered glyphs.
pub struct GlyphAtlas {
    fonts: Vec<Font>,
    font_size: f32,
    ascent: f32,
    cache: HashMap<(usize, char), GlyphInfo>,
    atlas_width: u32,
    atlas_height: u32,
    atlas_data: Vec<u8>,
    cursor_x: u16,
    cursor_y: u16,
    line_height: u16,
}

impl GlyphAtlas {
    pub fn new(font_size: f32) -> Self {
        let mut atlas_data = vec![0; 2048 * 2048];
        // Reserve pixel (0,0) as white for background quads
        atlas_data[0] = 255;
        Self {
            fonts: Vec::new(),
            font_size,
            ascent: 0.0,
            cache: HashMap::new(),
            atlas_width: 2048,
            atlas_height: 2048,
            atlas_data,
            cursor_x: 1, // Start after reserved pixel
            cursor_y: 0,
            line_height: (font_size * 1.2) as u16,
        }
    }

    /// Load a font from bytes.
    pub fn add_font(&mut self, font_data: &[u8]) -> Result<(), String> {
        let font = Font::from_bytes(font_data, fontdue::FontSettings::default())
            .map_err(|e| format!("Failed to load font: {:?}", e))?;

        // Compute ascent from the first font loaded
        if self.fonts.is_empty() {
            if let Some(line_metrics) = font.horizontal_line_metrics(self.font_size) {
                self.ascent = line_metrics.ascent;
            }
        }

        self.fonts.push(font);
        Ok(())
    }

    /// Load a font from a file path.
    pub fn load_font(&mut self, path: &Path) -> Result<(), String> {
        let data = std::fs::read(path)
            .map_err(|e| format!("Failed to read font file: {}", e))?;
        self.add_font(&data)
    }

    /// Load the default monospace font.
    pub fn load_default_font(&mut self) -> Result<(), String> {
        // Try to load a bundled default font or find one on the system
        #[cfg(target_os = "macos")]
        {
            let paths = [
                "/System/Library/Fonts/Monaco.ttf",
                "/System/Library/Fonts/SFNSMono.ttf",
                "/System/Library/Fonts/Menlo.ttc",
                "/Library/Fonts/JetBrainsMono-Regular.ttf",
            ];
            for path in &paths {
                if std::path::Path::new(path).exists() {
                    match self.load_font(std::path::Path::new(path)) {
                        Ok(()) => {
                            log::debug!("Loaded font: {} ({} fonts total)", path, self.fonts.len());
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
            ];
            for path in &paths {
                if std::path::Path::new(path).exists() {
                    match self.load_font(std::path::Path::new(path)) {
                        Ok(()) => {
                            log::debug!("Loaded font: {} ({} fonts total)", path, self.fonts.len());
                            return Ok(());
                        }
                        Err(e) => {
                            log::warn!("Failed to load font {}: {}", path, e);
                        }
                    }
                }
            }
        }

        // If no system font found, return an error
        Err("No suitable system font found. Please install a monospace font.".to_string())
    }

    /// Get or rasterize a glyph.
    pub fn get_glyph(&mut self, font_index: usize, c: char) -> Option<&GlyphInfo> {
        if self.fonts.is_empty() {
            return None;
        }

        let key = (font_index, c);
        if self.cache.contains_key(&key) {
            return self.cache.get(&key);
        }

        let font = &self.fonts.get(font_index)?;
        let (metrics, bitmap) = font.rasterize(c, self.font_size);

        let width = metrics.width as u16;
        let height = metrics.height as u16;

        // Check if we need a new row
        if self.cursor_x + width > self.atlas_width as u16 {
            self.cursor_x = 0;
            self.cursor_y += self.line_height;
        }

        // Check if atlas is full
        if self.cursor_y + height > self.atlas_height as u16 {
            log::warn!("Glyph atlas full, cannot cache more glyphs");
            return None;
        }

        // Write the glyph to the atlas
        let start_x = self.cursor_x as usize;
        let start_y = self.cursor_y as usize;
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
            x: self.cursor_x,
            y: self.cursor_y,
            width,
            height,
            x_offset: metrics.xmin as f32,
            y_offset: metrics.ymin as f32,
            advance: metrics.advance_width,
        };

        self.cursor_x += width + 1; // 1px padding
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
            self.line_height = (size * 1.2) as u16;
            self.cache.clear();
            self.atlas_data = vec![0; (self.atlas_width * self.atlas_height) as usize];
            self.atlas_data[0] = 255; // Re-reserve background pixel
            self.cursor_x = 1;
            self.cursor_y = 0;
        }
    }

    /// Get the cell dimensions for the current font size.
    pub fn cell_size(&self) -> (f32, f32) {
        if let Some(font) = self.fonts.first() {
            let (metrics, _) = font.rasterize('M', self.font_size);
            let width = metrics.advance_width;
            let height = self.font_size * 1.2;
            (width, height)
        } else {
            (8.0, 16.0)
        }
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
        self.fonts.len()
    }
}
