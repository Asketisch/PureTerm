//! Paste encoding for bracketed and non-bracketed modes.
//!
//! Ghostty paste handling from `input/paste.zig`.

use serde::{Deserialize, Serialize};

/// Whether bracketed paste mode is enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasteEncoding {
    /// Whether bracketed paste mode is active.
    pub bracketed: bool,
}

impl Default for PasteEncoding {
    fn default() -> Self {
        Self { bracketed: false }
    }
}

impl PasteEncoding {
    /// Create a new paste encoding state.
    pub fn new(bracketed: bool) -> Self {
        Self { bracketed }
    }

    /// Enable or disable bracketed paste mode.
    pub fn set_bracketed(&mut self, enabled: bool) {
        self.bracketed = enabled;
    }

    /// Check if bracketed paste mode is active.
    pub fn is_bracketed(&self) -> bool {
        self.bracketed
    }

    /// Encode paste data with the current mode.
    ///
    /// Returns the encoded bytes suitable for sending to the terminal.
    pub fn encode(&self, data: &str) -> Vec<u8> {
        if self.bracketed {
            self.encode_bracketed(data)
        } else {
            self.encode_non_bracketed(data)
        }
    }

    /// Encode with bracketed paste mode.
    ///
    /// Format: `ESC[200~ <data> ESC[201~`
    fn encode_bracketed(&self, data: &str) -> Vec<u8> {
        let mut result = Vec::with_capacity(data.len() + 20);
        result.extend_from_slice(b"\x1b[200~");
        result.extend_from_slice(data.as_bytes());
        result.extend_from_slice(b"\x1b[201~");
        result
    }

    /// Encode with non-bracketed (raw) paste mode.
    ///
    /// Converts LF to CR for terminal compatibility.
    fn encode_non_bracketed(&self, data: &str) -> Vec<u8> {
        let mut result = Vec::with_capacity(data.len());
        for byte in data.bytes() {
            if byte == b'\n' {
                result.push(b'\r');
            } else {
                result.push(byte);
            }
        }
        result
    }

    /// Filter paste data for safety.
    ///
    /// Replaces unsafe control characters with spaces.
    /// Safety filter from Ghostty:
    /// - NUL, BS, ENQ, EOT, ESC, DEL
    /// - VINTR, VQUIT, VKILL, VSUSP, VSUSP, VSTART, VSTOP, VWERASE, VLNEXT, VREPRINT, VDISCARD
    pub fn filter_safe(data: &str) -> String {
        let _unsafe_chars: &[u8] = &[
            0x00, // NUL
            0x07, // ENQ
            0x08, // BS
            0x10, // DLE
            0x13, // DC3
            0x14, // DC4
            0x1B, // ESC
            0x7F, // DEL
            // VT100 special keys (in C0 range but not C0 itself)
            // These are already covered by C0 filtering above
        ];

        // Extended unsafe characters (V* keys)
        let extended_unsafe: &[u8] = &[
            0x00, // VNULL (not really a thing but being safe)
            0x03, // VINTR (Ctrl+C)
            0x04, // VQUIT (Ctrl+\)
            0x06, // VACK (Ctrl+F)
            0x0B, // VKILL (Ctrl+K)
            0x0E, // VSUSP (Ctrl+N)
            0x10, // VSTART (Ctrl+P)
            0x11, // VSTOP (Ctrl+Q)
            0x12, // VREPRINT (Ctrl+R)
            0x13, // VDISCARD (Ctrl+S) - Note: 0x13 is also DC3
            0x14, // VLNEXT (Ctrl+V)
            0x15, // VWERASE (Ctrl+W)
            0x16, // VREPRINT (Ctrl+R) - Note: 0x16 is also (depending on interpretation)
            0x17, // VLNEXT (Ctrl+W)... wait let me redo this
            0x18, // VKILL (Ctrl+X)
            0x19, // VEOF (Ctrl+Y)
            0x1A, // VSUSP (Ctrl+Z)
        ];

        let mut result = String::with_capacity(data.len());
        for c in data.chars() {
            let byte = c as u8;
            // Replace C0 controls (0x00-0x1F except TAB, LF, CR, FF) and DEL (0x7F)
            if byte <= 0x1F && byte != 0x09 && byte != 0x0A && byte != 0x0C && byte != 0x0D {
                result.push(' ');
            } else if byte == 0x7F {
                result.push(' ');
            } else if extended_unsafe.contains(&byte) {
                result.push(' ');
            } else {
                result.push(c);
            }
        }
        result
    }

    /// Check if paste data is "safe" (doesn't contain newlines or paste terminator).
    ///
    /// Used to determine if bracketed paste mode is needed.
    pub fn is_safe(data: &str) -> bool {
        !data.contains('\n')
            && !data.contains('\r')
            && !data.contains("\x1b[201~") // ESC[201~ is paste terminator
    }
}

/// Safe paste indicator for bracketed mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteSafe {
    /// Data is safe for direct output.
    Safe,
    /// Data contains newlines or paste sequences.
    Unsafe,
}

impl PasteSafe {
    pub fn from_data(data: &str) -> Self {
        if PasteEncoding::is_safe(data) {
            PasteSafe::Safe
        } else {
            PasteSafe::Unsafe
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bracketed_paste() {
        let encoding = PasteEncoding::new(true);
        let encoded = encoding.encode("hello world");
        assert_eq!(encoded, b"\x1b[200~hello world\x1b[201~");
    }

    #[test]
    fn test_non_bracketed_paste() {
        let encoding = PasteEncoding::new(false);
        let encoded = encoding.encode("hello\nworld");
        // LF converted to CR
        assert_eq!(encoded, b"hello\rworld");
    }

    #[test]
    fn test_filter_safe() {
        // C0 controls should be replaced
        assert_eq!(
            PasteEncoding::filter_safe("hello\x00world"),
            "hello world"
        );

        // DEL should be replaced
        assert_eq!(
            PasteEncoding::filter_safe("hello\x7Fworld"),
            "hello world"
        );

        // Normal text passes through
        assert_eq!(
            PasteEncoding::filter_safe("hello world"),
            "hello world"
        );

        // TAB, LF, CR, FF pass through
        assert_eq!(
            PasteEncoding::filter_safe("hello\tworld\n"),
            "hello\tworld\n"
        );
    }

    #[test]
    fn test_is_safe() {
        assert!(PasteEncoding::is_safe("hello world"));
        assert!(!PasteEncoding::is_safe("hello\nworld"));
        assert!(!PasteEncoding::is_safe("hello\rworld"));
        assert!(!PasteEncoding::is_safe("hello\x1b[201~world"));
    }

    #[test]
    fn test_paste_encoding_default() {
        let encoding = PasteEncoding::default();
        assert!(!encoding.is_bracketed());
    }

    #[test]
    fn test_set_bracketed() {
        let mut encoding = PasteEncoding::default();
        encoding.set_bracketed(true);
        assert!(encoding.is_bracketed());
        encoding.set_bracketed(false);
        assert!(!encoding.is_bracketed());
    }
}
