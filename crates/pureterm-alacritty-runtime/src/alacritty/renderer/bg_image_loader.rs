//! Background image loading from disk.

use std::path::Path;

/// Decoded background image in RGBA8 format.
pub struct BgImageRaw {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Load a PNG or JPEG image file and convert to RGBA8 pixels.
pub fn load_background_image(path: &Path) -> Result<BgImageRaw, Box<dyn std::error::Error>> {
    let img = image::open(path)?.to_rgba8();
    let (w, h) = img.dimensions();
    Ok(BgImageRaw {
        data: img.into_raw(),
        width: w,
        height: h,
    })
}
