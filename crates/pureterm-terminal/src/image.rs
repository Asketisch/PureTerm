//! Kitty graphics protocol support.
//!
//! Implements the Kitty terminal graphics protocol for displaying images
//! inline in the terminal. Uses the APC escape sequence format:
//!   ESC _ G <control-data> ST
//!
//! Reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/

use std::collections::{HashMap, VecDeque};


/// Default maximum bytes for image storage (320MB as per Kitty default).
const DEFAULT_BYTE_LIMIT: usize = 320 * 1024 * 1024;

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
    /// Whether the image has a transparency channel.
    pub has_alpha: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// Raw RGBA pixels (format 32).
    Rgba,
    /// Raw RGB pixels (format 24).
    Rgb,
    /// PNG encoded (format 100).
    Png,
    /// Direct (uncompressed) RGBA (format 32).
    Direct,
}

impl Default for ImageFormat {
    fn default() -> Self {
        ImageFormat::Rgba
    }
}

/// Parsed Kitty graphics command.
#[derive(Debug, Clone)]
pub struct KittyGraphicsCommand {
    /// Action (t=transmit, p=place, q=query, d=delete).
    pub action: KittyAction,
    /// Whether data is compressed (zlib).
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
    /// More data follows (for chunked transfer).
    pub more_follows: bool,
    /// Transfer offset (for chunked transfer).
    pub offset: Option<u32>,
    /// Total data size (before compression).
    pub data_size: Option<u32>,
    /// Placement ID.
    pub placement_id: Option<u32>,
    /// Delete specific placements.
    pub delete_placements: Option<Vec<u32>>,
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
            more_follows: false,
            offset: None,
            data_size: None,
            placement_id: None,
            delete_placements: None,
            options: HashMap::new(),
        }
    }
}

/// State for chunked image transfer.
#[derive(Debug)]
pub struct ChunkedTransfer {
    /// Image ID being received.
    pub image_id: u32,
    /// Expected total size (uncompressed).
    pub total_size: u32,
    /// Current received bytes.
    pub received: Vec<u8>,
    /// Transfer offset.
    pub offset: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Image format.
    pub format: ImageFormat,
    /// Whether transfer is complete.
    pub complete: bool,
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
///
/// Sixel images are organized as rows of "sixel bands" (6 pixels tall each).
/// Each column in a band is described by a character whose 6 bits select
/// which of the 6 vertical pixels are drawn, using the current color register.
pub struct SixelParser {
    /// Color palette (up to 256 colors).
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

/// Per-pixel entry in the intermediate buffer: (color_register, pixel_mask_6bits).
/// Each column in a sixel band stores which of the 6 vertical pixels are set
/// and which color register they use.
type SixelPixel = (usize, u8); // (color_register_index, 6-bit mask)

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

    /// Parse a complete Sixel data stream and produce an image.
    ///
    /// Handles the full Sixel state machine:
    /// - `"` color/intensity definition (RGB or HLS)
    /// - `#` select color register
    /// - `!` repeat introducer followed by count and graphics character
    /// - `$` carriage return (move to beginning of current band)
    /// - `-` new line (advance to next 6-pixel band)
    /// - `?` through `~` graphics characters encoding 6 vertical pixels each
    pub fn parse_all(&mut self, data: &[u8]) -> SixelImage {
        self.reset();

        let mut current_color: usize = 0;
        // Each band row stores (color_register, 6-bit pixel mask) per column.
        // Multiple passes over the same column (via $) merge with OR.
        let mut bands: Vec<Vec<SixelPixel>> = Vec::new();
        let mut current_band: Vec<SixelPixel> = Vec::new();
        let mut col_pos: usize = 0;
        let mut max_width: usize = 0;

        let mut i = 0;
        while i < data.len() {
            let c = data[i];
            match c {
                b'#' => {
                    // Color introducer: `#Pc` selects register Pc;
                    // `#Pc;Pu;Px;Py;Pz` defines and selects (Pu=1 HLS, Pu=2 RGB).
                    i += 1;
                    let mut cparams: Vec<i32> = Vec::new();
                    let mut current_num = String::new();
                    while i < data.len() {
                        let pc = data[i];
                        match pc {
                            b'0'..=b'9' => {
                                current_num.push(pc as char);
                                i += 1;
                            }
                            b';' => {
                                cparams.push(current_num.parse::<i32>().unwrap_or(0));
                                current_num.clear();
                                i += 1;
                            }
                            _ => {
                                if !current_num.is_empty() {
                                    cparams.push(current_num.parse::<i32>().unwrap_or(0));
                                }
                                break;
                            }
                        }
                    }
                    if !cparams.is_empty() {
                        let reg = cparams[0].max(0) as usize;
                        if cparams.len() >= 5 && cparams[1] == 1 {
                            // HLS color: #Pc;1;Hh;Ll;Ss (H 0-360, L 0-100, S 0-100).
                            let h = (cparams[2] as f32 / 360.0).rem_euclid(1.0);
                            let l = (cparams[3] as f32 / 100.0).clamp(0.0, 1.0);
                            let s = (cparams[4] as f32 / 100.0).clamp(0.0, 1.0);
                            let (r, g, b) = hls_to_rgb(h, l, s);
                            self.set_color(reg, r, g, b);
                        } else if cparams.len() >= 5 && cparams[1] == 2 {
                            // RGB direct: #Pc;2;Rr;Gg;Bb (values 0-100 scale to 0-255).
                            let r = ((cparams[2].clamp(0, 100) as f32) * 255.0 / 100.0) as u8;
                            let g = ((cparams[3].clamp(0, 100) as f32) * 255.0 / 100.0) as u8;
                            let b = ((cparams[4].clamp(0, 100) as f32) * 255.0 / 100.0) as u8;
                            self.set_color(reg, r, g, b);
                        }
                        current_color = reg;
                    }
                    continue;
                }
                b'"' => {
                    // Raster Attributes (DECGRA): "Pan;Pad;Ph;Pv (pixel aspect + size hint).
                    // We accept and skip the parameters; we infer dimensions from the data.
                    i += 1;
                    while i < data.len() {
                        let pc = data[i];
                        if matches!(pc, b'0'..=b'9' | b';') {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    continue;
                }
                b'!' => {
                    // Repeat introducer: ! Pn Ch
                    i += 1;
                    let mut count_str = String::new();
                    while i < data.len() && data[i].is_ascii_digit() {
                        count_str.push(data[i] as char);
                        i += 1;
                    }
                    let count: u32 = count_str.parse().unwrap_or(1);
                    if i < data.len() && data[i] >= 0x3F && data[i] <= 0x7E {
                        let mask = decode_sixel_char(data[i]);
                        for _ in 0..count {
                            while current_band.len() <= col_pos {
                                current_band.push((0, 0));
                            }
                            let prev = current_band[col_pos];
                            current_band[col_pos] = (current_color, prev.1 | mask);
                            col_pos += 1;
                        }
                        max_width = max_width.max(col_pos);
                    }
                    i += 1;
                    continue;
                }
                b'$' => {
                    // CR within band: go back to column 0 for overlapping drawing
                    col_pos = 0;
                }
                b'-' => {
                    // New line: finalize current band, start next
                    if !current_band.is_empty() {
                        max_width = max_width.max(current_band.len());
                        bands.push(current_band.clone());
                    }
                    current_band.clear();
                    col_pos = 0;
                }
                0x3F..=0x7E => {
                    // Graphics character: 6-bit pixel mask
                    let mask = decode_sixel_char(c);
                    while current_band.len() <= col_pos {
                        current_band.push((0, 0));
                    }
                    let prev = current_band[col_pos];
                    current_band[col_pos] = (current_color, prev.1 | mask);
                    col_pos += 1;
                    max_width = max_width.max(col_pos);
                }
                _ => {
                    // Control characters ignored
                }
            }
            i += 1;
        }

        // Flush remaining band
        if !current_band.is_empty() {
            max_width = max_width.max(current_band.len());
            bands.push(current_band);
        }

        if bands.is_empty() || max_width == 0 {
            return SixelImage {
                id: 0,
                width: 0,
                height: 0,
                data: Vec::new(),
                color_palette: self.color_registers.clone(),
                cols: 0,
                rows: 0,
            };
        }

        // Convert bands to RGBA image
        let width = max_width;
        let height = bands.len() * 6;
        let mut img_data = vec![0u8; width * height * 4];

        for (band_idx, band) in bands.iter().enumerate() {
            for (col_idx, &(color_reg, mask)) in band.iter().enumerate() {
                if mask == 0 {
                    continue; // No pixels set in this column
                }
                let color = if color_reg < self.color_registers.len() {
                    self.color_registers[color_reg]
                } else {
                    [0, 0, 0]
                };
                // Each of the 6 bits in mask corresponds to a pixel row in the band
                for bit in 0..6 {
                    if mask & (1 << bit) != 0 {
                        let y = band_idx * 6 + bit;
                        let offset = (y * width + col_idx) * 4;
                        img_data[offset] = color[0];
                        img_data[offset + 1] = color[1];
                        img_data[offset + 2] = color[2];
                        img_data[offset + 3] = 255;
                    }
                }
            }
        }

        let cell_cols = ((width as u32) + 7) / 8;
        let cell_rows = ((height as u32) + 15) / 16;

        SixelImage {
            id: 0,
            width: width as u32,
            height: height as u32,
            data: img_data,
            color_palette: self.color_registers.clone(),
            cols: cell_cols,
            rows: cell_rows,
        }
    }

    /// Convert Sixel data to RGBA image (legacy API, prefer parse_all).
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

/// Decode a sixel character (0x3F-0x7E) into 6 vertical pixel bits.
/// The sixel character encodes 6 bits where bit 0 is top pixel.
/// Returns a u8 with bits 0-5 set for pixels that should be drawn.
fn decode_sixel_char(c: u8) -> u8 {
    // Sixel characters range from ? (0x3F) to ~ (0x7E)
    // The lower 6 bits represent 6 vertical pixels
    (c - 0x3F) & 0x3F
}

/// Convert HLS color values to RGB.
fn hls_to_rgb(h: f32, l: f32, s: f32) -> (u8, u8, u8) {
    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return (v, v, v);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    let hue_to_rgb = |t: f32| -> f32 {
        let mut t = t;
        if t < 0.0 { t += 1.0; }
        if t > 1.0 { t -= 1.0; }
        if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
        if t < 1.0 / 2.0 { return q; }
        if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
        p
    };

    let r = (hue_to_rgb(h + 1.0 / 3.0) * 255.0) as u8;
    let g = (hue_to_rgb(h) * 255.0) as u8;
    let b = (hue_to_rgb(h - 1.0 / 3.0) * 255.0) as u8;
    (r, g, b)
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
/// Format: key=value,key=value,...;payload_data
pub struct KittyGraphicsParser;

impl KittyGraphicsParser {
    /// Parse the control string portion of a Kitty graphics command.
    pub fn parse(control: &str, payload: &[u8]) -> Option<KittyGraphicsCommand> {
        let mut cmd = KittyGraphicsCommand::default();

        // Split control string into key=value pairs
        // Format can be: "a=t,f=32,s=100,v=100" or "a=t;data"
        let parts: Vec<&str> = control.split(',').collect();

        for part in parts {
            let kv: Vec<&str> = part.splitn(2, '=').collect();
            if kv.len() != 2 {
                continue;
            }
            let key = kv[0];
            let value = kv[1];

            match key {
                "a" => {
                    cmd.action = match value {
                        "t" | "T" | "transmit" => KittyAction::Transmit,
                        "p" | "P" | "place" => KittyAction::Place,
                        "q" | "Q" | "query" => KittyAction::Query,
                        "d" | "D" | "delete" => KittyAction::Delete,
                        "f" | "F" | "frame" => KittyAction::Frame,
                        _ => return None,
                    };
                }
                "f" => {
                    cmd.format = match value {
                        "32" => ImageFormat::Rgba,
                        "24" => ImageFormat::Rgb,
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
                "m" => {
                    cmd.more_follows = value == "1";
                }
                "O" => cmd.offset = value.parse().ok(),
                "S" => cmd.data_size = value.parse().ok(),
                "p" => cmd.placement_id = value.parse().ok(),
                "d" => {
                    // Delete specific placements (comma-separated IDs)
                    let ids: Option<Vec<u32>> = value
                        .split(':')
                        .map(|s| s.parse().ok())
                        .collect();
                    cmd.delete_placements = ids;
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

    /// Parse a complete Kitty graphics command string (control;payload).
    pub fn parse_command(data: &[u8]) -> Option<KittyGraphicsCommand> {
        // Find the semicolon separator between control and payload
        // Control is before semicolon, payload is base64-encoded data after
        let semicolon_pos = data.iter().position(|&b| b == b';');

        let (control, payload) = if let Some(pos) = semicolon_pos {
            (&data[..pos], &data[pos + 1..])
        } else {
            // No payload, treat all as control
            (&data[..], &[][..])
        };

        let control_str = String::from_utf8_lossy(control);

        // Decode base64 payload if present
        let decoded_payload = if !payload.is_empty() {
            Some(base64_decode(payload))
        } else {
            None
        };

        let payload_bytes = decoded_payload.unwrap_or_default();

        Self::parse(&control_str, &payload_bytes)
    }
}

/// Decode base64 data.
fn base64_decode(data: &[u8]) -> Vec<u8> {
    fn decode_char(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            b'=' => None, // Padding
            _ => None,
        }
    }

    let mut result = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;

    for &b in data {
        if b == b'\n' || b == b'\r' || b == b' ' || b == b'\t' {
            continue; // Skip whitespace
        }
        if let Some(value) = decode_char(b) {
            buffer = (buffer << 6) | (value as u32);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                result.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        } else if b != b'=' {
            // Invalid character in base64, but we still skip it
        }
    }

    result
}

/// Decompress zlib-compressed data.
fn decompress(data: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec(data).ok()
}

/// Decode PNG data to RGBA pixels.
pub fn decode_png(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = &buf[..info.buffer_size()];

    let width = info.width;
    let height = info.height;

    // Convert to RGBA if needed
    let rgba_data = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => {
            let mut rgba = vec![0u8; width as usize * height as usize * 4];
            for i in 0..(width as usize * height as usize) {
                rgba[i * 4] = bytes[i * 3];
                rgba[i * 4 + 1] = bytes[i * 3 + 1];
                rgba[i * 4 + 2] = bytes[i * 3 + 2];
                rgba[i * 4 + 3] = 255;
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = vec![0u8; width as usize * height as usize * 4];
            for i in 0..(width as usize * height as usize) {
                rgba[i * 4] = bytes[i * 2];
                rgba[i * 4 + 1] = bytes[i * 2];
                rgba[i * 4 + 2] = bytes[i * 2];
                rgba[i * 4 + 3] = bytes[i * 2 + 1];
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let mut rgba = vec![0u8; width as usize * height as usize * 4];
            for i in 0..(width as usize * height as usize) {
                rgba[i * 4] = bytes[i];
                rgba[i * 4 + 1] = bytes[i];
                rgba[i * 4 + 2] = bytes[i];
                rgba[i * 4 + 3] = 255;
            }
            rgba
        }
        _ => return None,
    };

    Some((width, height, rgba_data))
}

/// Decode raw RGB/RGBA pixel data.
fn decode_raw_pixels(
    data: &[u8],
    format: ImageFormat,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    match format {
        ImageFormat::Rgba => Some(data.to_vec()),
        ImageFormat::Rgb => {
            if data.len() < (width as usize * height as usize * 3) {
                return None;
            }
            let mut rgba = vec![0u8; width as usize * height as usize * 4];
            for i in 0..(width as usize * height as usize) {
                rgba[i * 4] = data[i * 3];
                rgba[i * 4 + 1] = data[i * 3 + 1];
                rgba[i * 4 + 2] = data[i * 3 + 2];
                rgba[i * 4 + 3] = 255;
            }
            Some(rgba)
        }
        _ => None,
    }
}

/// Manages terminal images (Kitty + Sixel) with LRU eviction.
pub struct ImageManager {
    images: HashMap<u32, TerminalImage>,
    sixel_images: HashMap<u32, SixelImage>,
    placements: HashMap<(u32, u32), Placement>, // (image_id, placement_id) -> Placement
    next_id: u32,
    byte_limit: usize,
    used_bytes: usize,
    /// Byte usage for sixel images.
    sixel_used_bytes: usize,
    /// LRU order for Kitty images (oldest first).
    lru_order: VecDeque<u32>,
    /// LRU order for Sixel images (oldest first).
    sixel_lru_order: VecDeque<u32>,
    /// Pending chunked transfers.
    chunked_transfers: HashMap<u32, ChunkedTransfer>,
}

#[derive(Debug, Clone)]
pub struct Placement {
    pub image_id: u32,
    pub placement_id: u32,
    pub x: u16,
    pub y: u16,
    pub z: i16,
    pub width: u16,
    pub height: u16,
}

impl ImageManager {
    pub fn new() -> Self {
        Self {
            images: HashMap::new(),
            sixel_images: HashMap::new(),
            placements: HashMap::new(),
            next_id: 1,
            byte_limit: DEFAULT_BYTE_LIMIT,
            used_bytes: 0,
            sixel_used_bytes: 0,
            lru_order: VecDeque::new(),
            sixel_lru_order: VecDeque::new(),
            chunked_transfers: HashMap::new(),
        }
    }

    /// Set the byte limit for image storage.
    pub fn set_byte_limit(&mut self, limit: usize) {
        self.byte_limit = limit;
        self.evict_if_needed();
    }

    /// Store a new image.
    pub fn store(&mut self, image: TerminalImage) -> u32 {
        let id = image.id;
        let size = image.data.len();

        // Evict if needed before adding
        while self.used_bytes + size > self.byte_limit && !self.images.is_empty() {
            self.evict_oldest();
        }

        // Remove existing image with same ID if present
        if let Some(old) = self.images.remove(&id) {
            self.used_bytes -= old.data.len();
            self.lru_order.retain(|&x| x != id);
        }

        self.images.insert(id, image);
        self.used_bytes += size;
        self.lru_order.push_back(id);
        id
    }

    /// Store a new Sixel image with byte tracking and LRU eviction.
    pub fn store_sixel(&mut self, image: SixelImage) -> u32 {
        let id = image.id;
        let size = image.data.len();

        // Evict oldest Sixel images if needed before adding
        while self.sixel_used_bytes + size > self.byte_limit && !self.sixel_images.is_empty() {
            self.evict_oldest_sixel();
        }

        // Remove existing image with same ID if present
        if let Some(old) = self.sixel_images.remove(&id) {
            self.sixel_used_bytes -= old.data.len();
            self.sixel_lru_order.retain(|&x| x != id);
        }

        self.sixel_images.insert(id, image);
        self.sixel_used_bytes += size;
        self.sixel_lru_order.push_back(id);
        id
    }

    /// Add a placement for an image.
    pub fn add_placement(&mut self, placement: Placement) {
        self.placements.insert((placement.image_id, placement.placement_id), placement);
    }

    /// Get a placement.
    pub fn get_placement(&self, image_id: u32, placement_id: u32) -> Option<&Placement> {
        self.placements.get(&(image_id, placement_id))
    }

    /// Get an image by ID.
    pub fn get(&self, id: u32) -> Option<&TerminalImage> {
        self.images.get(&id)
    }

    /// Get a Sixel image by ID.
    pub fn get_sixel(&self, id: u32) -> Option<&SixelImage> {
        self.sixel_images.get(&id)
    }

    /// Get a mutable reference to an image by ID.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut TerminalImage> {
        self.images.get_mut(&id)
    }

    /// Update an image's position and z-index.
    pub fn update_image_position(&mut self, id: u32, col: Option<u32>, row: Option<u32>, z_index: Option<i32>) {
        if let Some(img) = self.images.get_mut(&id) {
            if let Some(c) = col {
                img.col = c;
            }
            if let Some(r) = row {
                img.row = r;
            }
            if let Some(z) = z_index {
                img.z_index = z;
            }
        }
    }

    /// Remove an image by ID.
    pub fn remove(&mut self, id: u32) -> Option<TerminalImage> {
        if let Some(image) = self.images.remove(&id) {
            self.used_bytes -= image.data.len();
            self.lru_order.retain(|&x| x != id);
            // Remove all placements for this image
            self.placements.retain(|(img_id, _), _| *img_id != id);
            Some(image)
        } else {
            None
        }
    }

    /// Remove a Sixel image by ID.
    pub fn remove_sixel(&mut self, id: u32) -> Option<SixelImage> {
        if let Some(image) = self.sixel_images.remove(&id) {
            self.sixel_used_bytes -= image.data.len();
            self.sixel_lru_order.retain(|&x| x != id);
            Some(image)
        } else {
            None
        }
    }

    /// Remove all images at a given cell position.
    pub fn remove_at(&mut self, row: u32, col: u32) {
        let to_remove: Vec<u32> = self.images.values()
            .filter(|img| {
                img.row <= row && row < img.row + img.rows
                    && img.col <= col && col < img.col + img.cols
            })
            .map(|img| img.id)
            .collect();

        for id in to_remove {
            self.remove(id);
        }
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

    /// Get all placements.
    pub fn all_placements(&self) -> impl Iterator<Item = &Placement> {
        self.placements.values()
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
        self.placements.clear();
        self.used_bytes = 0;
        self.sixel_used_bytes = 0;
        self.lru_order.clear();
        self.sixel_lru_order.clear();
    }

    /// Get total bytes used by images (Kitty + Sixel).
    pub fn used_bytes(&self) -> usize {
        self.used_bytes + self.sixel_used_bytes
    }

    /// Get byte limit.
    pub fn byte_limit(&self) -> usize {
        self.byte_limit
    }

    /// Process a chunked transfer.
    pub fn process_chunked_transfer(&mut self, cmd: &KittyGraphicsCommand) -> Option<TerminalImage> {
        let image_id = cmd.image_id?;

        if cmd.action == KittyAction::Delete {
            // Remove the pending transfer
            self.chunked_transfers.remove(&image_id);
            return None;
        }

        // Start a new transfer or continue existing
        let is_new = !self.chunked_transfers.contains_key(&image_id);

        if is_new {
            // Start new transfer
            let total_size = cmd.data_size?;
            let width = cmd.width?;
            let height = cmd.height?;
            let format = cmd.format;

            let transfer = ChunkedTransfer {
                image_id,
                total_size,
                received: Vec::new(),
                offset: cmd.offset.unwrap_or(0),
                width,
                height,
                format,
                complete: false,
            };

            self.chunked_transfers.insert(image_id, transfer);
        }

        // Append data at the correct offset
        let data = cmd.data.as_ref()?;
        let offset = cmd.offset.unwrap_or(0) as usize;

        {
            let transfer = self.chunked_transfers.get_mut(&image_id)?;

            // Ensure we have enough space
            while transfer.received.len() < offset + data.len() {
                transfer.received.push(0);
            }
            transfer.received[offset..offset + data.len()].copy_from_slice(data);

            // Check if transfer is complete
            if !cmd.more_follows || transfer.received.len() >= transfer.total_size as usize {
                transfer.complete = true;
            }
        }

        // If complete, decode the image
        let transfer = self.chunked_transfers.get(&image_id)?;
        if transfer.complete {
            let received = transfer.received.clone();
            let format = transfer.format;
            let width = transfer.width;
            let height = transfer.height;

            self.chunked_transfers.remove(&image_id);
            return self.decode_and_store_image(
                image_id,
                &received,
                format,
                width,
                height,
            );
        }

        None
    }

    /// Decode and store an image from raw data.
    fn decode_and_store_image(
        &mut self,
        id: u32,
        data: &[u8],
        format: ImageFormat,
        width: u32,
        height: u32,
    ) -> Option<TerminalImage> {
        // Decompress if needed
        let decompressed = if format == ImageFormat::Png || data.len() > 10000 {
            // Try zlib decompression first for non-PNG
            decompress(data).unwrap_or_else(|| data.to_vec())
        } else {
            data.to_vec()
        };

        // Handle PNG specially
        if format == ImageFormat::Png {
            let (w, h, rgba) = decode_png(&decompressed)?;
            let image = TerminalImage {
                id,
                width: w,
                height: h,
                data: rgba,
                cols: (w + 7) / 8, // Approximate cell size
                rows: (h + 15) / 16,
                col: 0,
                row: 0,
                format,
                z_index: 0,
                has_alpha: true,
            };
            self.store(image.clone());
            return Some(image);
        }

        // Handle raw RGB/RGBA
        let rgba = if format == ImageFormat::Png {
            // Already decoded above
            return Some(TerminalImage {
                id,
                width,
                height,
                data: decompressed,
                cols: (width + 7) / 8,
                rows: (height + 15) / 16,
                col: 0,
                row: 0,
                format,
                z_index: 0,
                has_alpha: true,
            });
        } else {
            decode_raw_pixels(&decompressed, format, width, height)?
        };

        let has_alpha = format == ImageFormat::Rgba;

        let image = TerminalImage {
            id,
            width,
            height,
            data: rgba,
            cols: (width + 7) / 8,
            rows: (height + 15) / 16,
            col: 0,
            row: 0,
            format,
            z_index: 0,
            has_alpha,
        };

        self.store(image.clone());
        Some(image)
    }

    /// Store an image directly from a command.
    pub fn store_from_command(&mut self, cmd: &KittyGraphicsCommand) -> Option<TerminalImage> {
        let image_id = cmd.image_id?;
        let data = cmd.data.as_ref()?;
        let width = cmd.width?;
        let height = cmd.height?;

        self.decode_and_store_image(image_id, data, cmd.format, width, height)
    }

    /// Remove images by ID list (for delete action).
    pub fn delete_images(&mut self, ids: &[u32]) {
        for &id in ids {
            self.remove(id);
        }
    }

    /// Remove all images.
    pub fn delete_all(&mut self) {
        self.clear();
    }

    /// Evict the oldest image when at capacity.
    fn evict_oldest(&mut self) {
        if let Some(oldest_id) = self.lru_order.pop_front() {
            if let Some(image) = self.images.remove(&oldest_id) {
                self.used_bytes -= image.data.len();
                // Remove placements for this image
                self.placements.retain(|(img_id, _), _| *img_id != oldest_id);
            }
        }
    }

    /// Evict the oldest Sixel image when at capacity.
    fn evict_oldest_sixel(&mut self) {
        if let Some(oldest_id) = self.sixel_lru_order.pop_front() {
            if let Some(image) = self.sixel_images.remove(&oldest_id) {
                self.sixel_used_bytes -= image.data.len();
                self.placements.retain(|(img_id, _), _| *img_id != oldest_id);
            }
        }
    }

    /// Evict images until under capacity.
    fn evict_if_needed(&mut self) {
        while self.used_bytes > self.byte_limit && !self.images.is_empty() {
            self.evict_oldest();
        }
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
    fn test_parse_with_compression() {
        let cmd = KittyGraphicsParser::parse("a=t,f=32,s=10,v=10,i=1,o=z", &[]).unwrap();
        assert!(cmd.compressed);
    }

    #[test]
    fn test_parse_chunked_transfer() {
        let cmd = KittyGraphicsParser::parse("a=t,f=32,s=100,v=100,i=1,S=1000,m=1", &[]).unwrap();
        assert!(cmd.more_follows);
        assert_eq!(cmd.data_size, Some(1000));
    }

    #[test]
    fn test_image_manager_lru() {
        let mut mgr = ImageManager::new();
        // Each image is 100x100x4 = 40,000 bytes
        // Set limit to 80,000 bytes (exactly 2 images)
        mgr.set_byte_limit(80_000);

        // Store images
        let img1 = TerminalImage {
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
            has_alpha: true,
        };
        mgr.store(img1);

        let img2 = TerminalImage {
            id: 2,
            width: 100,
            height: 100,
            data: vec![0; 100 * 100 * 4],
            cols: 10,
            rows: 5,
            col: 0,
            row: 0,
            format: ImageFormat::Rgba,
            z_index: 0,
            has_alpha: true,
        };
        mgr.store(img2);

        // Both should fit (80,000 bytes used, 80,000 limit)
        assert!(mgr.get(1).is_some());
        assert!(mgr.get(2).is_some());

        // Add image 3 - this exceeds the limit, so image 1 (oldest) should be evicted
        let img3 = TerminalImage {
            id: 3,
            width: 100,
            height: 100,
            data: vec![0; 100 * 100 * 4],
            cols: 10,
            rows: 5,
            col: 0,
            row: 0,
            format: ImageFormat::Rgba,
            z_index: 0,
            has_alpha: true,
        };
        mgr.store(img3);

        // Image 1 should be evicted (oldest), but images 2 and 3 should remain
        assert!(mgr.get(1).is_none());
        assert!(mgr.get(2).is_some());
        assert!(mgr.get(3).is_some());
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
            has_alpha: true,
        };
        mgr.store(img);
        assert!(mgr.get(1).is_some());
        assert_eq!(mgr.images_in_region(0, 0, 5, 10).len(), 1);
        mgr.remove(1);
        assert!(mgr.get(1).is_none());
    }

    #[test]
    fn test_base64_decode() {
        // "Hello" in base64
        let encoded = b"SGVsbG8";
        let decoded = base64_decode(encoded);
        assert_eq!(decoded, b"Hello");
    }

    // ── Sixel parser tests ──────────────────────────────────────────────────

    #[test]
    fn parse_sixel_minimal_image() {
        // `#0;2;100;0;0` defines color 0 as full red (RGB 0-100 scale).
        // `#0` selects color 0.
        // `??` two empty sixel cells (mask 0).
        // `!10~` repeats `~` (mask 0x3F = all 6 rows) 10 times.
        // `$` resets x to 0; `-` advances to next band (we ignore the empty band).
        let mut parser = SixelParser::new();
        let image = parser.parse_all(b"#0;2;100;0;0#0??!10~$-");
        // 2 empty + 10 filled columns = 12 wide; one band = 6 tall.
        assert_eq!(image.width, 12);
        assert!(image.height >= 6);
        assert!(!image.data.is_empty());

        // Check that at least one filled column has red pixels.
        // Column 2 (first filled column), row 0 -> offset (0 * width + 2) * 4
        let off = (0 * image.width as usize + 2) * 4;
        assert_eq!(image.data[off], 255, "expected red channel = 255 at filled pixel");
        assert_eq!(image.data[off + 1], 0, "expected green channel = 0 at filled pixel");
        assert_eq!(image.data[off + 2], 0, "expected blue channel = 0 at filled pixel");
        assert_eq!(image.data[off + 3], 255, "expected alpha = 255 at filled pixel");

        // Empty column 0, row 0 should have alpha 0 (transparent).
        let off0 = (0 * image.width as usize + 0) * 4;
        assert_eq!(image.data[off0 + 3], 0, "expected empty pixel to be transparent");
    }

    #[test]
    fn parse_sixel_rejects_garbage() {
        let mut parser = SixelParser::new();
        // Empty input -> empty image (width=0, height=0).
        let empty = parser.parse_all(b"");
        assert_eq!(empty.width, 0);
        assert_eq!(empty.height, 0);
        assert!(empty.data.is_empty());

        // Pure non-sixel control bytes -> still empty.
        let mut parser2 = SixelParser::new();
        let garbage = parser2.parse_all(b"\x01\x02\x03 \t\n");
        assert_eq!(garbage.width, 0);
        assert_eq!(garbage.height, 0);
    }

    #[test]
    fn hls_to_rgb_known_triples() {
        // L=0 -> black regardless of H/S.
        let (r, g, b) = hls_to_rgb(0.5, 0.0, 1.0);
        assert_eq!((r, g, b), (0, 0, 0));

        // L=1.0 -> white.
        let (r, g, b) = hls_to_rgb(0.5, 1.0, 1.0);
        assert_eq!((r, g, b), (255, 255, 255));

        // S=0, L=0.5 -> mid grey.
        let (r, g, b) = hls_to_rgb(0.0, 0.5, 0.0);
        assert!(r > 120 && r < 135);
        assert_eq!(r, g);
        assert_eq!(g, b);

        // Pure red: H=0, L=0.5, S=1.0.
        let (r, g, b) = hls_to_rgb(0.0, 0.5, 1.0);
        assert_eq!(r, 255);
        assert_eq!(g, 0);
        assert_eq!(b, 0);
    }

    #[test]
    fn parse_sixel_hls_color() {
        // Define color register 1 as HLS pure red (H=0, L=50, S=100), then draw a column.
        let mut parser = SixelParser::new();
        let image = parser.parse_all(b"#1;1;0;50;100#1~");
        assert_eq!(image.width, 1);
        assert!(image.height >= 6);
        let off = 0;
        assert_eq!(image.data[off], 255);
        assert_eq!(image.data[off + 1], 0);
        assert_eq!(image.data[off + 2], 0);
    }

    #[test]
    fn parse_sixel_advance_band() {
        // Two bands of one filled column -> height 12.
        let mut parser = SixelParser::new();
        let image = parser.parse_all(b"#0;2;0;100;0#0~-~");
        assert_eq!(image.width, 1);
        assert_eq!(image.height, 12);
        // Top pixel should be green.
        assert_eq!(image.data[1], 255);
    }

    #[test]
    fn store_sixel_assigns_id_and_tracks_bytes() {
        let mut mgr = ImageManager::new();
        let img = SixelImage {
            id: 42,
            width: 4,
            height: 4,
            data: vec![0u8; 4 * 4 * 4],
            color_palette: Vec::new(),
            cols: 1,
            rows: 1,
        };
        let id = mgr.store_sixel(img);
        assert_eq!(id, 42);
        assert!(mgr.get_sixel(42).is_some());
        assert!(mgr.used_bytes() >= 4 * 4 * 4);
    }
}
