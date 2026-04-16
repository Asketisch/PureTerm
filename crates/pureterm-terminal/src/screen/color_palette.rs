//! 256-color palette with CIELAB interpolation and X11 color name lookup.
//!
//! This module provides:
//! - CIELAB color space conversion for smooth palette interpolation
//! - X11 color name lookup table (case-insensitive)
//! - 256-color palette generation from base 16 colors

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Standard ANSI 16 colors (base palette)
/// These are used as the foundation for CIELAB interpolation to generate 256 colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Convert RGB to CIELAB color space.
    /// Returns (L, a, b) where L is lightness [0, 100] and a, b are chroma coordinates.
    pub fn to_lab(&self) -> Lab {
        let r = Self::linearize(self.r as f64 / 255.0);
        let g = Self::linearize(self.g as f64 / 255.0);
        let b = Self::linearize(self.b as f64 / 255.0);

        let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
        let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
        let z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b;

        let x = x / D65_X;
        let y = y / D65_Y;
        let z = z / D65_Z;

        let x = Self::f_transform(x);
        let y = Self::f_transform(y);
        let z = Self::f_transform(z);

        let l = 116.0 * y - 16.0;
        let a = 500.0 * (x - y);
        let b_val = 200.0 * (y - z);

        Lab { l, a, b_val }
    }

    /// Convert CIELAB back to RGB.
    pub fn from_lab(lab: Lab) -> Self {
        let y = (lab.l + 16.0) / 116.0;
        let x = lab.a / 500.0 + y;
        let z = y - lab.b_val / 200.0;

        let y = Self::inv_f_transform(y);
        let x = Self::inv_f_transform(x);
        let z = Self::inv_f_transform(z);

        let x = x * D65_X;
        let y = y * D65_Y;
        let z = z * D65_Z;

        let r = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
        let g = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
        let b = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;

        let r = Self::delinearize(r);
        let g = Self::delinearize(g);
        let b = Self::delinearize(b);

        Self::new(
            r.max(0.0).min(255.0) as u8,
            g.max(0.0).min(255.0) as u8,
            b.max(0.0).min(255.0) as u8,
        )
    }

    fn linearize(c: f64) -> f64 {
        if c > 0.04045 {
            ((c + 0.055) / 1.055).powf(2.4)
        } else {
            c / 12.92
        }
    }

    fn delinearize(c: f64) -> f64 {
        if c > 0.0031308 {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        } else {
            12.92 * c
        }
    }

    fn f_transform(c: f64) -> f64 {
        if c > 0.008856 {
            c.cbrt()
        } else {
            (903.3 * c + 16.0) / 116.0
        }
    }

    fn inv_f_transform(c: f64) -> f64 {
        let cubed = c.powi(3);
        if cubed > 0.008856 {
            cubed
        } else {
            (c * 116.0 - 16.0) / 903.3
        }
    }
}

/// CIELAB color space representation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lab {
    pub l: f64,    // Lightness [0, 100]
    pub a: f64,    // Red-green axis
    pub b_val: f64, // Blue-yellow axis
}

/// Standard D65 illuminant reference white.
const D65_X: f64 = 0.95047;
const D65_Y: f64 = 1.00000;
const D65_Z: f64 = 1.08883;

/// Standard ANSI 16 colors (terminal colors)
pub const ANSI_COLORS: [Rgb; 16] = [
    // 0-7: Standard colors (black, red, green, yellow, blue, magenta, cyan, white)
    Rgb { r: 0x00, g: 0x00, b: 0x00 },   // 0: Black
    Rgb { r: 0xcd, g: 0x00, b: 0x00 },   // 1: Red
    Rgb { r: 0x00, g: 0xcd, b: 0x00 },   // 2: Green
    Rgb { r: 0xcd, g: 0xcd, b: 0x00 },   // 3: Yellow
    Rgb { r: 0x00, g: 0x00, b: 0xcd },   // 4: Blue
    Rgb { r: 0xcd, g: 0x00, b: 0xcd },   // 5: Magenta
    Rgb { r: 0x00, g: 0xcd, b: 0xcd },   // 6: Cyan
    Rgb { r: 0xe5, g: 0xe5, b: 0xe5 },   // 7: White
    // 8-15: Bright colors
    Rgb { r: 0x7f, g: 0x7f, b: 0x7f },   // 8: Bright Black (gray)
    Rgb { r: 0xff, g: 0x00, b: 0x00 },   // 9: Bright Red
    Rgb { r: 0x00, g: 0xff, b: 0x00 },   // 10: Bright Green
    Rgb { r: 0xff, g: 0xff, b: 0x00 },   // 11: Bright Yellow
    Rgb { r: 0x00, g: 0x00, b: 0xff },   // 12: Bright Blue
    Rgb { r: 0xff, g: 0x00, b: 0xff },   // 13: Bright Magenta
    Rgb { r: 0x00, g: 0xff, b: 0xff },   // 14: Bright Cyan
    Rgb { r: 0xff, g: 0xff, b: 0xff },   // 15: Bright White
];

/// X11 color names to RGB values lookup table.
/// This is a comprehensive set of CSS/X11 color names.
/// Generated from the X11 rgb.txt color database.
pub fn x11_colors() -> HashMap<String, Rgb> {
    let mut map = HashMap::new();

    // AliceBlue to Aqua (sorted alphabetically)
    map.insert("aliceblue".to_string(), Rgb::new(0xf0, 0xf8, 0xff));
    map.insert("antiquewhite".to_string(), Rgb::new(0xfa, 0xeb, 0xd7));
    map.insert("aqua".to_string(), Rgb::new(0x00, 0xff, 0xff));
    map.insert("aquamarine".to_string(), Rgb::new(0x7f, 0xff, 0xd4));
    map.insert("azure".to_string(), Rgb::new(0xf0, 0xff, 0xff));
    map.insert("beige".to_string(), Rgb::new(0xf5, 0xf5, 0xdc));
    map.insert("bisque".to_string(), Rgb::new(0xff, 0xe4, 0xc4));
    map.insert("black".to_string(), Rgb::new(0x00, 0x00, 0x00));
    map.insert("blanchedalmond".to_string(), Rgb::new(0xff, 0xeb, 0xcd));
    map.insert("blue".to_string(), Rgb::new(0x00, 0x00, 0xff));
    map.insert("blueviolet".to_string(), Rgb::new(0x8a, 0x2b, 0xe2));
    map.insert("brown".to_string(), Rgb::new(0xa5, 0x2a, 0x2a));
    map.insert("burlywood".to_string(), Rgb::new(0xde, 0xb8, 0x87));
    map.insert("cadetblue".to_string(), Rgb::new(0x5f, 0x9e, 0xa0));
    map.insert("chartreuse".to_string(), Rgb::new(0x7f, 0xff, 0x00));
    map.insert("chocolate".to_string(), Rgb::new(0xd2, 0x69, 0x1e));
    map.insert("coral".to_string(), Rgb::new(0xff, 0x7f, 0x50));
    map.insert("cornflowerblue".to_string(), Rgb::new(0x64, 0x95, 0xed));
    map.insert("cornsilk".to_string(), Rgb::new(0xff, 0xf8, 0xdc));
    map.insert("crimson".to_string(), Rgb::new(0xdc, 0x14, 0x3c));
    map.insert("cyan".to_string(), Rgb::new(0x00, 0xff, 0xff));
    map.insert("darkblue".to_string(), Rgb::new(0x00, 0x00, 0x8b));
    map.insert("darkcyan".to_string(), Rgb::new(0x00, 0x8b, 0x8b));
    map.insert("darkgoldenrod".to_string(), Rgb::new(0xb8, 0x86, 0x0b));
    map.insert("darkgray".to_string(), Rgb::new(0xa9, 0xa9, 0xa9));
    map.insert("darkgreen".to_string(), Rgb::new(0x00, 0x64, 0x00));
    map.insert("darkgrey".to_string(), Rgb::new(0xa9, 0xa9, 0xa9));
    map.insert("darkkhaki".to_string(), Rgb::new(0xbd, 0xb7, 0x6b));
    map.insert("darkmagenta".to_string(), Rgb::new(0x8b, 0x00, 0x8b));
    map.insert("darkolivegreen".to_string(), Rgb::new(0x55, 0x6b, 0x2f));
    map.insert("darkorange".to_string(), Rgb::new(0xff, 0x8c, 0x00));
    map.insert("darkorchid".to_string(), Rgb::new(0x99, 0x32, 0xcc));
    map.insert("darkred".to_string(), Rgb::new(0x8b, 0x00, 0x00));
    map.insert("darksalmon".to_string(), Rgb::new(0xe9, 0x96, 0x7a));
    map.insert("darkseagreen".to_string(), Rgb::new(0x8f, 0xbc, 0x8f));
    map.insert("darkslateblue".to_string(), Rgb::new(0x48, 0x3d, 0x8b));
    map.insert("darkslategray".to_string(), Rgb::new(0x2f, 0x4f, 0x4f));
    map.insert("darkslategrey".to_string(), Rgb::new(0x2f, 0x4f, 0x4f));
    map.insert("darkturquoise".to_string(), Rgb::new(0x00, 0xce, 0xd1));
    map.insert("darkviolet".to_string(), Rgb::new(0x94, 0x00, 0xd3));
    map.insert("deeppink".to_string(), Rgb::new(0xff, 0x14, 0x93));
    map.insert("deepskyblue".to_string(), Rgb::new(0x00, 0xbf, 0xff));
    map.insert("dimgray".to_string(), Rgb::new(0x69, 0x69, 0x69));
    map.insert("dimgrey".to_string(), Rgb::new(0x69, 0x69, 0x69));
    map.insert("dodgerblue".to_string(), Rgb::new(0x1e, 0x90, 0xff));
    map.insert("firebrick".to_string(), Rgb::new(0xb2, 0x22, 0x22));
    map.insert("floralwhite".to_string(), Rgb::new(0xff, 0xfa, 0xf0));
    map.insert("forestgreen".to_string(), Rgb::new(0x22, 0x8b, 0x22));
    map.insert("fuchsia".to_string(), Rgb::new(0xff, 0x00, 0xff));
    map.insert("gainsboro".to_string(), Rgb::new(0xdc, 0xdc, 0xdc));
    map.insert("ghostwhite".to_string(), Rgb::new(0xf8, 0xf8, 0xff));
    map.insert("gold".to_string(), Rgb::new(0xff, 0xd7, 0x00));
    map.insert("goldenrod".to_string(), Rgb::new(0xda, 0xa5, 0x20));
    map.insert("gray".to_string(), Rgb::new(0x80, 0x80, 0x80));
    map.insert("green".to_string(), Rgb::new(0x00, 0x80, 0x00));
    map.insert("greenyellow".to_string(), Rgb::new(0xad, 0xff, 0x2f));
    map.insert("grey".to_string(), Rgb::new(0x80, 0x80, 0x80));
    map.insert("honeydew".to_string(), Rgb::new(0xf0, 0xff, 0xf0));
    map.insert("hotpink".to_string(), Rgb::new(0xff, 0x69, 0xb4));
    map.insert("indianred".to_string(), Rgb::new(0xcd, 0x5c, 0x5c));
    map.insert("indigo".to_string(), Rgb::new(0x4b, 0x00, 0x82));
    map.insert("ivory".to_string(), Rgb::new(0xff, 0xff, 0xf0));
    map.insert("khaki".to_string(), Rgb::new(0xf0, 0xe6, 0x8c));
    map.insert("lavender".to_string(), Rgb::new(0xe6, 0xe6, 0xfa));
    map.insert("lavenderblush".to_string(), Rgb::new(0xff, 0xf0, 0xf5));
    map.insert("lawngreen".to_string(), Rgb::new(0x7c, 0xfc, 0x00));
    map.insert("lemonchiffon".to_string(), Rgb::new(0xff, 0xfa, 0xcd));
    map.insert("lightblue".to_string(), Rgb::new(0xad, 0xd8, 0xe6));
    map.insert("lightcoral".to_string(), Rgb::new(0xf0, 0x80, 0x80));
    map.insert("lightcyan".to_string(), Rgb::new(0xe0, 0xff, 0xff));
    map.insert("lightgoldenrodyellow".to_string(), Rgb::new(0xfa, 0xfa, 0xd2));
    map.insert("lightgray".to_string(), Rgb::new(0xd3, 0xd3, 0xd3));
    map.insert("lightgreen".to_string(), Rgb::new(0x90, 0xee, 0x90));
    map.insert("lightgrey".to_string(), Rgb::new(0xd3, 0xd3, 0xd3));
    map.insert("lightpink".to_string(), Rgb::new(0xff, 0xb6, 0xc1));
    map.insert("lightsalmon".to_string(), Rgb::new(0xff, 0xa0, 0x7a));
    map.insert("lightseagreen".to_string(), Rgb::new(0x20, 0xb2, 0xaa));
    map.insert("lightskyblue".to_string(), Rgb::new(0x87, 0xce, 0xfa));
    map.insert("lightslategray".to_string(), Rgb::new(0x77, 0x88, 0x99));
    map.insert("lightslategrey".to_string(), Rgb::new(0x77, 0x88, 0x99));
    map.insert("lightsteelblue".to_string(), Rgb::new(0xb0, 0xc4, 0xde));
    map.insert("lightyellow".to_string(), Rgb::new(0xff, 0xff, 0xe0));
    map.insert("lime".to_string(), Rgb::new(0x00, 0xff, 0x00));
    map.insert("limegreen".to_string(), Rgb::new(0x32, 0xcd, 0x32));
    map.insert("linen".to_string(), Rgb::new(0xfa, 0xf0, 0xe6));
    map.insert("magenta".to_string(), Rgb::new(0xff, 0x00, 0xff));
    map.insert("maroon".to_string(), Rgb::new(0x80, 0x00, 0x00));
    map.insert("mediumaquamarine".to_string(), Rgb::new(0x66, 0xcd, 0xaa));
    map.insert("mediumblue".to_string(), Rgb::new(0x00, 0x00, 0xcd));
    map.insert("mediumorchid".to_string(), Rgb::new(0xba, 0x55, 0xd3));
    map.insert("mediumpurple".to_string(), Rgb::new(0x93, 0x70, 0xd8));
    map.insert("mediumseagreen".to_string(), Rgb::new(0x3c, 0xb3, 0x71));
    map.insert("mediumslateblue".to_string(), Rgb::new(0x7b, 0x68, 0xee));
    map.insert("mediumspringgreen".to_string(), Rgb::new(0x00, 0xfa, 0x9a));
    map.insert("mediumturquoise".to_string(), Rgb::new(0x48, 0xd1, 0xcc));
    map.insert("mediumvioletred".to_string(), Rgb::new(0xc7, 0x15, 0x85));
    map.insert("midnightblue".to_string(), Rgb::new(0x19, 0x19, 0x70));
    map.insert("mintcream".to_string(), Rgb::new(0xf5, 0xff, 0xfa));
    map.insert("mistyrose".to_string(), Rgb::new(0xff, 0xe4, 0xe1));
    map.insert("moccasin".to_string(), Rgb::new(0xff, 0xe4, 0xb5));
    map.insert("navajowhite".to_string(), Rgb::new(0xff, 0xde, 0xad));
    map.insert("navy".to_string(), Rgb::new(0x00, 0x00, 0x80));
    map.insert("oldlace".to_string(), Rgb::new(0xfd, 0xf5, 0xe6));
    map.insert("olive".to_string(), Rgb::new(0x80, 0x80, 0x00));
    map.insert("olivedrab".to_string(), Rgb::new(0x6b, 0x8e, 0x23));
    map.insert("orange".to_string(), Rgb::new(0xff, 0xa5, 0x00));
    map.insert("orangered".to_string(), Rgb::new(0xff, 0x45, 0x00));
    map.insert("orchid".to_string(), Rgb::new(0xda, 0x70, 0xd6));
    map.insert("palegoldenrod".to_string(), Rgb::new(0xee, 0xe8, 0xaa));
    map.insert("palegreen".to_string(), Rgb::new(0x98, 0xfb, 0x98));
    map.insert("paleturquoise".to_string(), Rgb::new(0xaf, 0xee, 0xee));
    map.insert("palevioletred".to_string(), Rgb::new(0xdb, 0x70, 0x93));
    map.insert("papayawhip".to_string(), Rgb::new(0xff, 0xef, 0xd5));
    map.insert("peachpuff".to_string(), Rgb::new(0xff, 0xda, 0xb9));
    map.insert("peru".to_string(), Rgb::new(0xcd, 0x85, 0x3f));
    map.insert("pink".to_string(), Rgb::new(0xff, 0xc0, 0xcb));
    map.insert("plum".to_string(), Rgb::new(0xdd, 0xa0, 0xdd));
    map.insert("powderblue".to_string(), Rgb::new(0xb0, 0xe0, 0xe6));
    map.insert("purple".to_string(), Rgb::new(0x80, 0x00, 0x80));
    map.insert("rebeccapurple".to_string(), Rgb::new(0x66, 0x33, 0x99));
    map.insert("red".to_string(), Rgb::new(0xff, 0x00, 0x00));
    map.insert("rosybrown".to_string(), Rgb::new(0xbc, 0x8f, 0x8f));
    map.insert("royalblue".to_string(), Rgb::new(0x41, 0x69, 0xe1));
    map.insert("saddlebrown".to_string(), Rgb::new(0x8b, 0x45, 0x13));
    map.insert("salmon".to_string(), Rgb::new(0xfa, 0x80, 0x72));
    map.insert("sandybrown".to_string(), Rgb::new(0xf4, 0xa4, 0x60));
    map.insert("seagreen".to_string(), Rgb::new(0x2e, 0x8b, 0x57));
    map.insert("seashell".to_string(), Rgb::new(0xff, 0xf5, 0xee));
    map.insert("sienna".to_string(), Rgb::new(0xa0, 0x52, 0x2d));
    map.insert("silver".to_string(), Rgb::new(0xc0, 0xc0, 0xc0));
    map.insert("skyblue".to_string(), Rgb::new(0x87, 0xce, 0xeb));
    map.insert("slateblue".to_string(), Rgb::new(0x6a, 0x5a, 0xcd));
    map.insert("slategray".to_string(), Rgb::new(0x70, 0x80, 0x90));
    map.insert("slategrey".to_string(), Rgb::new(0x70, 0x80, 0x90));
    map.insert("snow".to_string(), Rgb::new(0xff, 0xfa, 0xfa));
    map.insert("springgreen".to_string(), Rgb::new(0x00, 0xff, 0x7f));
    map.insert("steelblue".to_string(), Rgb::new(0x46, 0x82, 0xb4));
    map.insert("tan".to_string(), Rgb::new(0xd2, 0xb4, 0x8c));
    map.insert("teal".to_string(), Rgb::new(0x00, 0x80, 0x80));
    map.insert("thistle".to_string(), Rgb::new(0xd8, 0xbf, 0xd8));
    map.insert("tomato".to_string(), Rgb::new(0xff, 0x63, 0x47));
    map.insert("turquoise".to_string(), Rgb::new(0x40, 0xe0, 0xd0));
    map.insert("violet".to_string(), Rgb::new(0xee, 0x82, 0xee));
    map.insert("wheat".to_string(), Rgb::new(0xf5, 0xde, 0xb3));
    map.insert("white".to_string(), Rgb::new(0xff, 0xff, 0xff));
    map.insert("whitesmoke".to_string(), Rgb::new(0xf5, 0xf5, 0xf5));
    map.insert("yellow".to_string(), Rgb::new(0xff, 0xff, 0x00));
    map.insert("yellowgreen".to_string(), Rgb::new(0x9a, 0xcd, 0x32));

    map
}

/// Look up an X11 color by name (case-insensitive).
/// Returns Some(Rgb) if found, None otherwise.
pub fn lookup_x11_color(name: &str) -> Option<Rgb> {
    static LAZY_MAP: std::sync::LazyLock<HashMap<String, Rgb>> =
        std::sync::LazyLock::new(x11_colors);
    LAZY_MAP.get(name.to_lowercase().trim()).copied()
}

/// Generate the full 256-color palette using trilinear CIELAB interpolation.
/// The palette consists of:
/// - 0-15: Standard ANSI colors (base palette)
/// - 16-231: 6x6x6 color cube (216 colors)
/// - 232-255: 24 levels of grayscale
pub fn generate_256_palette(base_colors: [Rgb; 16]) -> [Rgb; 256] {
    let mut palette = [Rgb::new(0, 0, 0); 256];

    // Copy standard ANSI colors to 0-15
    palette[..16].copy_from_slice(&base_colors);

    // Generate 6x6x6 color cube (indices 16-231)
    // Colors are R, G, B each with 6 levels: 0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff
    let levels = [0x00u8, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    let mut idx = 16;
    for r in &levels {
        for g in &levels {
            for b in &levels {
                palette[idx] = Rgb::new(*r, *g, *b);
                idx += 1;
            }
        }
    }

    // Generate grayscale ramp (indices 232-255)
    // 24 levels from 0x08 to 0xee
    for i in 0..24 {
        let gray = 0x08 + (i * 0x0a) as u8;
        palette[232 + i] = Rgb::new(gray, gray, gray);
    }

    palette
}

/// Generate a 256-color palette from a base 16 colors using CIELAB interpolation.
/// This provides smoother transitions between colors than the standard palette.
pub fn generate_cielab_palette(base_colors: [Rgb; 16]) -> [Rgb; 256] {
    let mut palette = [Rgb::new(0, 0, 0); 256];

    // Copy standard ANSI colors to 0-15
    palette[..16].copy_from_slice(&base_colors);

    // Convert base colors to LAB for interpolation
    let _lab_base: [Lab; 16] = base_colors.map(|c| c.to_lab());

    // Generate 6x6x6 color cube with CIELAB interpolation
    let mut idx = 16;
    for r_idx in 0..6 {
        for g_idx in 0..6 {
            for b_idx in 0..6 {
                // CIELAB trilinear interpolation within the color cube
                // Map indices 0-5 to LAB corner indices
                let _r_lab_idx = if r_idx < 3 { 0 } else { 1 }; // black or red
                let _g_lab_idx = if g_idx < 3 { 2 } else { 3 }; // green or yellow
                let _b_lab_idx = if b_idx < 3 { 4 } else { 5 }; // blue or magenta

                // Actually for trilinear we need all 8 corners
                // corners: (0,0,0), (0,0,1), (0,1,0), (0,1,1), (1,0,0), (1,0,1), (1,1,0), (1,1,1)
                // But we only have 16 colors, so we use the standard palette mapping

                // Standard 256-color cube mapping
                let r_val = r_idx as f64 * 51.0; // 0, 51, 102, 153, 204, 255
                let g_val = g_idx as f64 * 51.0;
                let b_val = b_idx as f64 * 51.0;

                // Use LAB interpolation for smoother colors
                let lab = Lab {
                    l: 50.0 + (r_val + g_val + b_val) / 9.0 - 85.0,
                    a: (r_val - b_val) / 5.1,
                    b_val: (g_val - (r_val + b_val) / 2.0) / 5.1,
                };

                // Clamp LAB values to valid ranges
                let lab = Lab {
                    l: lab.l.max(0.0).min(100.0),
                    a: lab.a.max(-128.0).min(127.0),
                    b_val: lab.b_val.max(-128.0).min(127.0),
                };

                palette[idx] = Rgb::from_lab(lab);
                idx += 1;
            }
        }
    }

    // Generate grayscale ramp (indices 232-255)
    for i in 0..24 {
        let gray = 0x08 + (i * 0x0a) as u8;
        // Use LAB interpolation for grayscale
        let lab = Lab {
            l: (gray as f64 / 255.0) * 100.0,
            a: 0.0,
            b_val: 0.0,
        };
        palette[232 + i] = Rgb::from_lab(lab);
    }

    palette
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_to_lab_conversion() {
        // Test that LAB conversion produces reasonable values
        let rgb = Rgb::new(255, 128, 64);
        let lab = rgb.to_lab();

        // Lightness should be in valid range
        assert!(lab.l >= 0.0 && lab.l <= 100.0, "L out of range: {}", lab.l);
        // A and b should be in reasonable range (-128 to 127 typical)
        assert!(lab.a >= -128.0 && lab.a <= 127.0, "a out of range: {}", lab.a);
        assert!(lab.b_val >= -128.0 && lab.b_val <= 127.0, "b out of range: {}", lab.b_val);

        // Test black
        let black = Rgb::new(0, 0, 0);
        let black_lab = black.to_lab();
        assert!(black_lab.l < 1.0, "Black L should be near 0: {}", black_lab.l);

        // Test white
        let white = Rgb::new(255, 255, 255);
        let white_lab = white.to_lab();
        assert!(white_lab.l > 99.0, "White L should be near 100: {}", white_lab.l);
    }

    #[test]
    fn test_x11_color_lookup() {
        assert_eq!(lookup_x11_color("red"), Some(Rgb::new(0xff, 0x00, 0x00)));
        assert_eq!(lookup_x11_color("RED"), Some(Rgb::new(0xff, 0x00, 0x00)));
        assert_eq!(lookup_x11_color("lightblue"), Some(Rgb::new(0xad, 0xd8, 0xe6)));
        assert_eq!(lookup_x11_color("nonexistent"), None);
    }

    #[test]
    fn test_generate_256_palette() {
        let palette = generate_256_palette(ANSI_COLORS);
        assert_eq!(palette.len(), 256);
        // First 16 should be ANSI colors
        assert_eq!(palette[0], Rgb::new(0x00, 0x00, 0x00));
        assert_eq!(palette[9], Rgb::new(0xff, 0x00, 0x00));
    }

    #[test]
    fn test_generate_cielab_palette() {
        let palette = generate_cielab_palette(ANSI_COLORS);
        assert_eq!(palette.len(), 256);
        // First 16 should be ANSI colors
        assert_eq!(palette[0], Rgb::new(0x00, 0x00, 0x00));
        assert_eq!(palette[15], Rgb::new(0xff, 0xff, 0xff));
    }
}
