//! Bracketed paste input handling.

/// Wraps pasted content with bracketed paste escape sequences if the
/// terminal has enabled bracketed paste mode (CSI ? 2004 h).
pub struct BracketedPasteHandler;

impl BracketedPasteHandler {
    /// Wrap content for bracketed paste mode.
    /// Returns the content wrapped with ESC[200~ and ESC[201~ if enabled.
    pub fn wrap(content: &[u8], bracketed_paste_enabled: bool) -> Vec<u8> {
        if !bracketed_paste_enabled {
            return content.to_vec();
        }

        let mut result = Vec::with_capacity(content.len() + 12);
        result.extend_from_slice(b"\x1b[200~");
        result.extend_from_slice(content);
        result.extend_from_slice(b"\x1b[201~");
        result
    }

    /// Process incoming data to detect and extract bracketed paste content.
    /// Returns a list of paste events and raw input segments.
    pub fn process_input(data: &[u8]) -> Vec<InputSegment> {
        let mut segments = Vec::new();
        let mut i = 0;
        let mut raw_start = 0;

        while i < data.len() {
            // Look for ESC[200~ (paste start)
            if i + 6 <= data.len()
                && data[i] == 0x1b
                && data[i + 1] == b'['
                && data[i + 2] == b'2'
                && data[i + 3] == b'0'
                && data[i + 4] == b'0'
                && data[i + 5] == b'~'
            {
                // Emit any raw data before paste start
                if i > raw_start {
                    segments.push(InputSegment::Raw(data[raw_start..i].to_vec()));
                }

                // Find paste end: ESC[201~
                let paste_start = i + 6;
                let mut paste_end = paste_start;
                let mut found_end = false;

                while paste_end + 6 <= data.len() {
                    if data[paste_end] == 0x1b
                        && data[paste_end + 1] == b'['
                        && data[paste_end + 2] == b'2'
                        && data[paste_end + 3] == b'0'
                        && data[paste_end + 4] == b'1'
                        && data[paste_end + 5] == b'~'
                    {
                        found_end = true;
                        break;
                    }
                    paste_end += 1;
                }

                if found_end {
                    segments.push(InputSegment::Paste(data[paste_start..paste_end].to_vec()));
                    i = paste_end + 6;
                    raw_start = i;
                } else {
                    // No end marker found — treat rest as incomplete paste
                    segments.push(InputSegment::Paste(data[paste_start..].to_vec()));
                    return segments;
                }
            } else {
                i += 1;
            }
        }

        if raw_start < data.len() {
            segments.push(InputSegment::Raw(data[raw_start..].to_vec()));
        }

        segments
    }
}

/// A segment of input data, either raw keystrokes or pasted content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSegment {
    /// Raw keyboard input.
    Raw(Vec<u8>),
    /// Content from a bracketed paste.
    Paste(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_bracketed() {
        let content = b"hello world";
        let wrapped = BracketedPasteHandler::wrap(content, true);
        assert!(wrapped.starts_with(b"\x1b[200~"));
        assert!(wrapped.ends_with(b"\x1b[201~"));
        assert_eq!(&wrapped[6..wrapped.len() - 6], content);
    }

    #[test]
    fn test_wrap_not_bracketed() {
        let content = b"hello world";
        let wrapped = BracketedPasteHandler::wrap(content, false);
        assert_eq!(wrapped, content);
    }

    #[test]
    fn test_process_input_paste() {
        let input = b"abc\x1b[200~pasted\x1b[201~def".to_vec();
        let segments = BracketedPasteHandler::process_input(&input);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0], InputSegment::Raw(b"abc".to_vec()));
        assert_eq!(segments[1], InputSegment::Paste(b"pasted".to_vec()));
        assert_eq!(segments[2], InputSegment::Raw(b"def".to_vec()));
    }

    #[test]
    fn test_process_input_no_paste() {
        let input = b"just regular input";
        let segments = BracketedPasteHandler::process_input(input);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], InputSegment::Raw(b"just regular input".to_vec()));
    }
}
