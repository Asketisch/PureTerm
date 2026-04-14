//! Character set handling for ANSI terminal.
//!
//! Supports G0-G3 character sets with locking shift (LS) and single shift (SS).

/// Character set index (G0-G3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharsetIndex {
    G0,
    G1,
    G2,
    G3,
}

/// Standard character sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    /// US ASCII (default).
    ASCII,
    /// DEC Special Graphics (box drawing characters).
    DecSpecialGraphics,
    /// UK variant (British).
    UK,
    /// Portuguese.
    Portuguese,
    /// Norwegian/Danish.
    NorwegianDanish,
    /// Swedish.
    Swedish,
    /// German.
    German,
    /// French.
    French,
    /// Italian.
    Italian,
    /// Spanish.
    Spanish,
    /// Dutch.
    Dutch,
    /// Swiss.
    Swiss,
    /// ISO 8859-1 (Latin-1).
    ISO88591,
    /// User-defined mapping (not implemented).
    UserDefined,
}

/// Map ASCII code to DEC Special Graphics character.
/// Returns the mapped character if applicable.
pub fn dec_graphics_char(byte: u8) -> Option<char> {
    match byte {
        0x5F => Some('\u{FFFD}'), // PAD - not used
        0x60 => Some('\u{25C6}'), // ` -> diamond
        0x61 => Some('\u{25C7}'), // a -> white diamond
        0x62 => Some('\u{25B2}'), // b -> up arrow
        0x63 => Some('\u{25BC}'), // c -> down arrow
        0x64 => Some('\u{25B6}'), // d -> right arrow
        0x65 => Some('\u{25C0}'), // e -> left arrow
        0x66 => Some('\u{2510}'), // f -> top-right corner
        0x67 => Some('\u{2514}'), // g -> bottom-left corner
        0x68 => Some('\u{2534}'), // h -> bottom-T
        0x69 => Some('\u{252C}'), // i -> top-T
        0x6A => Some('\u{2524}'), // j -> left-T
        0x6B => Some('\u{253C}'), // k -> cross
        0x6C => Some('\u{251C}'), // l -> left-right
        0x6D => Some('\u{2500}'), // m -> horizontal line
        0x6E => Some('\u{2502}'), // n -> vertical line
        0x6F => Some('\u{249C}'), // o -> quadrant lower-left
        0x70 => Some('\u{2460}'), // p -> circled digit 1
        0x71 => Some('\u{2461}'), // q -> circled digit 2
        0x72 => Some('\u{2462}'), // r -> circled digit 3
        0x73 => Some('\u{2463}'), // s -> circled digit 4
        0x74 => Some('\u{2320}'), // t -> top horizontal segment
        0x75 => Some('\u{2321}'), // u -> bottom horizontal segment
        0x76 => Some('\u{23DA}'), // v -> floor cross
        0x7E => Some('\u{2248}'), // ~ -> approximately
        _ => None,
    }
}

/// Map a character through the specified charset.
pub fn map_char(charset: Charset, c: char) -> char {
    match charset {
        Charset::DecSpecialGraphics => {
            // Only map printable ASCII in graphics mode
            if c as u8 >= 0x5F && c as u8 <= 0x7E {
                dec_graphics_char(c as u8).unwrap_or(c)
            } else {
                c
            }
        }
        _ => c, // No mapping for other charsets
    }
}

/// Get charset from designation code.
/// Intermediate bytes determine the specific variant.
pub fn charset_from_code(code: u8, _intermediates: &[u8]) -> Charset {
    match code {
        b'A' => Charset::ISO88591, // Also used for UK in some contexts
        b'0' => Charset::DecSpecialGraphics,
        b'1' => Charset::ISO88591,
        b'2' => Charset::ISO88591,
        _ => Charset::ASCII,
    }
}
