//! Kitty graphics protocol support.
//!
//! Implements the Kitty terminal graphics protocol for displaying images
//! inline in the terminal. Uses the APC escape sequence format:
//!   ESC _ G <control-data> ST
//!
//! Reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/

use std::collections::HashMap;

/// A placed image in the terminal grid.
#[derive(Debug, Clone)]
pub struct TerminalImage {
    /// Unique image ID.
    pub id: u32,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Raw pixel data (RGBA).
    pub data: Vec<u8>,
    /// Number of columns the image occupies.
    pub cols: u32,
    /// Number of rows the image occupies.
    pub rows: u32,
    /// Top-left column position.
    pub col: u32,
    /// Top-left row position.
    pub row: u32,
    /// Image format.
    pub format: ImageFormat,
    /// Z-index for layering.
    pub z_index: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// Raw RGBA pixels.
    Rgba,
    /// PNG encoded.
    Png,
}

/// Parsed Kitty graphics command.
#[derive(Debug, Clone)]
pub struct KittyGraphicsCommand {
    /// Action (t=transmit, p=place, q=query, d=delete).
    pub action: KittyAction,
    /// Whether data is compressed.
    pub compressed: bool,
    /// Image format.
    pub format: ImageFormat,
    /// Image ID.
    pub image_id: Option<u32>,
    /// Image number (for chunked transmission).
    pub image_number: Option<u32>,
    /// Width and height in pixels.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Position on screen (x, y).
    pub pos_x: Option<u32>,
    pub pos_y: Option<u32>,
    /// Size in cells (columns, rows).
    pub cell_cols: Option<u32>,
    pub cell_rows: Option<u32>,
    /// Crop region (x, y, w, h).
    pub crop: Option<(u32, u32, u32, u32)>,
    /// Z-index.
    pub z_index: Option<i32>,
    /// Raw data payload.
    pub data: Option<Vec<u8>>,
    /// Additional options.
    pub options: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KittyAction {
    /// Transmit image data.
    Transmit,
    /// Place a previously transmitted image.
    Place,
    /// Query image state.
    Query,
    /// Delete images.
    Delete,
    /// Frame control (for animations).
    Frame,
}

impl Default for KittyGraphicsCommand {
    fn default() -> Self {
        Self {
            action: KittyAction::Transmit,
            compressed: false,
            format: ImageFormat::Rgba,
            image_id: None,
            image_number: None,
            width: None,
            height: None,
            pos_x: None,
            pos_y: None,
            cell_cols: None,
            cell_rows: None,
            crop: None,
            z_index: None,
            data: None,
            options: HashMap::new(),
        }
    }
}

// ── Sixel Image Support ──────────────────────────────────────────────────────

/// Sixel image representation.
/// Sixel encodes images using 6-pixel tall horizontal strips.
#[derive(Debug, Clone)]
pub struct SixelImage {
    /// Unique image ID.
    pub id: u32,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Raw pixel data (RGBA).
    pub data: Vec<u8>,
    /// Color palette (up to 256 colors).
    pub color_palette: Vec<[u8; 3]>,
    /// Cell dimensions for positioning.
    pub cols: u32,
    pub rows: u32,
}

/// Sixel parser state machine.
pub struct SixelParser {
    /// Color palette (16 or 256 colors).
    color_registers: Vec<[u8; 3]>,
    /// Max color register number (0-15 for 16-color mode).
    max_register: usize,
    /// Current repeat count.
    repeat_count: u32,
    /// Current character code.
    current_char: u8,
    /// DECSIXEL parameters.
    params: Vec<i32>,
    /// Raw pixel strips (6 pixels tall each).
    strips: Vec<Vec<u8>>,
    /// Current strip width in pixels.
    strip_width: usize,
}

impl SixelParser {
    pub fn new() -> Self {
        let mut color_registers = Vec::with_capacity(256);
        // Initialize with standard 16 ANSI colors
        for i in 0..16 {
            color_registers.push([
                (i as u8 * 16).wrapping_add(8),
                (i as u8 * 16).wrapping_add(8),
                (i as u8 * 16).wrapping_add(8),
            ]);
        }
        Self {
            color_registers,
            max_register: 15,
            repeat_count: 1,
            current_char: 0,
            params: Vec::new(),
            strips: Vec::new(),
            strip_width: 0,
        }
    }

    /// Reset the parser state.
    pub fn reset(&mut self) {
        self.color_registers.clear();
        for i in 0..16 {
            self.color_registers.push([
                (i as u8 * 16).wrapping_add(8),
                (i as u8 * 16).wrapping_add(8),
                (i as u8 * 16).wrapping_add(8),
            ]);
        }
        self.max_register = 15;
        self.repeat_count = 1;
        self.current_char = 0;
        self.params.clear();
        self.strips.clear();
        self.strip_width = 0;
    }

    /// Set color palette size via DECSIXEL parameter.
    pub fn set_palette_size(&mut self, size: usize) {
        self.max_register = size.saturating_sub(1).min(255);
        self.color_registers.clear();
        self.color_registers.resize(size, [128, 128, 128]);
    }

    /// Set color in palette via DECSIXEL parameter.
    pub fn set_color(&mut self, reg: usize, r: u8, g: u8, b: u8) {
        if reg <= 255 {
            while self.color_registers.len() <= reg {
                self.color_registers.push([128, 128, 128]);
            }
            self.color_registers[reg] = [r, g, b];
        }
    }

    /// Parse a Sixel data character (range ! ~ or \x80-\xFF).
    pub fn parse_char(&mut self, c: u8) -> Option<SixelAction> {
        match c {
            0x21..=0x7E => {
                // Regular graphics characters ! (33) to ~ (126)
                // Each character encodes 6 vertical pixels
                self.current_char = c;
                Some(SixelAction::Graphics(self.current_char))
            }
            0x80..=0xFF => {
                // Repeat introducer or shift
                self.current_char = c;
                Some(SixelAction::Shift)
            }
            _ => None,
        }
    }

    /// Process a parameter setting command.
    pub fn set_param(&mut self, param: i32) {
        self.params.push(param);
    }

    /// Finalize parameters and prepare for data.
    pub fn finalize_params(&mut self) {
        // Process stored parameters
        // Format: DCS P s p1 p2 p3 p4 p5 p6 p7 q
        // p1: aspect or - (no change)
        // p2-p5: width, height or - (no change)
        // p6: palette size
        // p7: transparency
        self.strip_width = 0;
        self.strips.clear();
    }

    /// Get current repeat count.
    pub fn get_repeat(&self) -> u32 {
        self.repeat_count
    }

    /// Set repeat count.
    pub fn set_repeat(&mut self, count: u32) {
        self.repeat_count = count;
    }

    /// Add a graphics strip row (6 pixels tall).
    pub fn add_strip(&mut self, strip: Vec<u8>) {
        let w = strip.len();
        if self.strip_width == 0 {
            self.strip_width = w;
        }
        self.strips.push(strip);
    }

    /// Convert Sixel data to RGBA image.
    pub fn to_image(&self, id: u32, cell_cols: u32, cell_rows: u32) -> SixelImage {
        let height = self.strips.len() * 6;
        let width = self.strip_width;

        let mut data = vec![0u8; width * height * 4];

        for (row_idx, strip) in self.strips.iter().enumerate() {
            for (col_idx, &color_idx) in strip.iter().enumerate() {
                let reg = (color_idx & 0x3F) as usize; // 6-bit color index
                let color = if reg < self.color_registers.len() {
                    self.color_registers[reg]
                } else {
                    [0, 0, 0]
                };

                // Fill 6 vertical pixels for this strip
                for dy in 0..6 {
                    let y = row_idx * 6 + dy;
                    if y < height {
                        let offset = (y * width + col_idx) * 4;
                        data[offset] = color[0];
                        data[offset + 1] = color[1];
                        data[offset + 2] = color[2];
                        data[offset + 3] = 255;
                    }
                }
            }
        }

        SixelImage {
            id,
            width: width as u32,
            height: height as u32,
            data,
            color_palette: self.color_registers.clone(),
            cols: cell_cols,
            rows: cell_rows,
        }
    }
}

impl Default for SixelParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Actions from Sixel parser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SixelAction {
    /// Graphics character for a strip.
    Graphics(u8),
    /// Shift to next color register set.
    Shift,
    /// Set raster attributes.
    RasterAttr,
    /// Color register operation.
    ColorRegister,
    /// String terminator.
    StringTerm,
}

/// Parser for Kitty graphics protocol commands.
pub struct KittyGraphicsParser;

impl KittyGraphicsParser {
    /// Parse the control string portion of a Kitty graphics command.
    /// Format: key=value,key=value,...;payload_data
    pub fn parse(control: &str, payload: &[u8]) -> Option<KittyGraphicsCommand> {
        let mut cmd = KittyGraphicsCommand::default();

        for part in control.split(',') {
            let kv: Vec<&str> = part.splitn(2, '=').collect();
            if kv.len() != 2 {
                continue;
            }
            let key = kv[0];
            let value = kv[1];

            match key {
                "a" => {
                    cmd.action = match value {
                        "t" | "T" => KittyAction::Transmit,
                        "p" | "P" => KittyAction::Place,
                        "q" | "Q" => KittyAction::Query,
                        "d" | "D" => KittyAction::Delete,
                        "f" | "F" => KittyAction::Frame,
                        _ => return None,
                    };
                }
                "f" => {
                    cmd.format = match value {
                        "32" => ImageFormat::Rgba,
                        "100" => ImageFormat::Png,
                        _ => ImageFormat::Rgba,
                    };
                }
                "o" => {
                    cmd.compressed = value == "z";
                }
                "i" => cmd.image_id = value.parse().ok(),
                "I" => cmd.image_number = value.parse().ok(),
                "s" => cmd.width = value.parse().ok(),
                "v" => cmd.height = value.parse().ok(),
                "x" => cmd.pos_x = value.parse().ok(),
                "y" => cmd.pos_y = value.parse().ok(),
                "c" => cmd.cell_cols = value.parse().ok(),
                "r" => cmd.cell_rows = value.parse().ok(),
                "z" => cmd.z_index = value.parse().ok(),
                "S" => {
                    // Crop: S=width,height,x,y
                    let parts: Vec<u32> = value.split('x')
                        .chain(std::iter::once(""))
                        .flat_map(|s| s.parse())
                        .collect();
                    // Simplified crop parsing
                }
                _ => {
                    cmd.options.insert(key.to_string(), value.to_string());
                }
            }
        }

        if !payload.is_empty() {
            cmd.data = Some(payload.to_vec());
        }

        Some(cmd)
    }
}

/// Manages terminal images (Kitty + Sixel).
pub struct ImageManager {
    images: HashMap<u32, TerminalImage>,
    sixel_images: HashMap<u32, SixelImage>,
    next_id: u32,
}

impl ImageManager {
    pub fn new() -> Self {
        Self {
            images: HashMap::new(),
            sixel_images: HashMap::new(),
            next_id: 1,
        }
    }

    /// Store a new image.
    pub fn store(&mut self, image: TerminalImage) -> u32 {
        let id = image.id;
        self.images.insert(id, image);
        id
    }

    /// Store a new Sixel image.
    pub fn store_sixel(&mut self, image: SixelImage) -> u32 {
        let id = image.id;
        self.sixel_images.insert(id, image);
        id
    }

    /// Get an image by ID.
    pub fn get(&self, id: u32) -> Option<&TerminalImage> {
        self.images.get(&id)
    }

    /// Get a Sixel image by ID.
    pub fn get_sixel(&self, id: u32) -> Option<&SixelImage> {
        self.sixel_images.get(&id)
    }

    /// Remove an image by ID.
    pub fn remove(&mut self, id: u32) -> Option<TerminalImage> {
        self.images.remove(&id)
    }

    /// Remove a Sixel image by ID.
    pub fn remove_sixel(&mut self, id: u32) -> Option<SixelImage> {
        self.sixel_images.remove(&id)
    }

    /// Remove all images at a given cell position.
    pub fn remove_at(&mut self, row: u32, col: u32) {
        self.images.retain(|_, img| {
            !(img.row <= row && row < img.row + img.rows && img.col <= col && col < img.col + img.cols)
        });
    }

    /// Get all images overlapping a region.
    pub fn images_in_region(&self, row: u32, col: u32, rows: u32, cols: u32) -> Vec<&TerminalImage> {
        self.images.values()
            .filter(|img| {
                img.row < row + rows && img.row + img.rows > row
                    && img.col < col + cols && img.col + img.cols > col
            })
            .collect()
    }

    /// Allocate the next image ID.
    pub fn allocate_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Clear all images (Kitty and Sixel).
    pub fn clear(&mut self) {
        self.images.clear();
        self.sixel_images.clear();
    }
}

impl Default for ImageManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_transmit_command() {
        let cmd = KittyGraphicsParser::parse("a=t,f=32,s=100,v=100,i=1", &[]).unwrap();
        assert_eq!(cmd.action, KittyAction::Transmit);
        assert_eq!(cmd.format, ImageFormat::Rgba);
        assert_eq!(cmd.width, Some(100));
        assert_eq!(cmd.height, Some(100));
        assert_eq!(cmd.image_id, Some(1));
    }

    #[test]
    fn test_image_manager() {
        let mut mgr = ImageManager::new();
        let img = TerminalImage {
            id: 1,
            width: 100,
            height: 100,
            data: vec![0; 100 * 100 * 4],
            cols: 10,
            rows: 5,
            col: 0,
            row: 0,
            format: ImageFormat::Rgba,
            z_index: 0,
        };
        mgr.store(img);
        assert!(mgr.get(1).is_some());
        assert_eq!(mgr.images_in_region(0, 0, 5, 10).len(), 1);
        mgr.remove(1);
        assert!(mgr.get(1).is_none());
    }
}
