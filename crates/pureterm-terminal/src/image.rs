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

/// Manages terminal images.
pub struct ImageManager {
    images: HashMap<u32, TerminalImage>,
    next_id: u32,
}

impl ImageManager {
    pub fn new() -> Self {
        Self {
            images: HashMap::new(),
            next_id: 1,
        }
    }

    /// Store a new image.
    pub fn store(&mut self, image: TerminalImage) -> u32 {
        let id = image.id;
        self.images.insert(id, image);
        id
    }

    /// Get an image by ID.
    pub fn get(&self, id: u32) -> Option<&TerminalImage> {
        self.images.get(&id)
    }

    /// Remove an image by ID.
    pub fn remove(&mut self, id: u32) -> Option<TerminalImage> {
        self.images.remove(&id)
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

    /// Clear all images.
    pub fn clear(&mut self) {
        self.images.clear();
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
