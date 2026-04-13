use bytes::Bytes;

use crate::middleware::SessionMiddleware;

/// Current state of Zmodem detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZmodemState {
    /// No transfer in progress.
    Normal,
    /// A transfer start marker was just detected.
    TransferStart,
    /// A transfer is currently in progress.
    Transferring,
    /// A transfer end marker was just detected.
    TransferEnd,
}

/// Middleware that detects Zmodem file-transfer sequences and replaces binary
/// data with a human-readable placeholder so the terminal doesn't get flooded
/// with raw bytes.
pub struct ZmodemMiddleware {
    in_transfer: bool,
}

/// Zmodem start marker: `**\x18B0` (the ZRQINIT header often appears after `rz\r`).
const ZMODEM_START: &[u8] = b"**\x18B0";

/// Zmodem end marker: `**\x18B0` followed by `OO` (over-and-out).
const ZMODEM_END: &[u8] = b"**\x18B0OO";

impl ZmodemMiddleware {
    pub fn new() -> Self {
        Self {
            in_transfer: false,
        }
    }

    /// Scan `data` for Zmodem markers and return the detected state alongside
    /// the (possibly replaced) bytes.
    fn detect_and_replace(&mut self, data: &[u8]) -> (ZmodemState, Bytes) {
        // Check for end marker first (it is a superset of the start marker).
        if self.in_transfer && contains_subsequence(data, ZMODEM_END) {
            self.in_transfer = false;
            let msg = b"[Zmodem transfer in progress...]";
            return (ZmodemState::TransferEnd, Bytes::from_static(msg));
        }

        // Check for start marker.
        if contains_subsequence(data, ZMODEM_START) || contains_subsequence(data, b"rz\r") {
            self.in_transfer = true;
            let msg = b"[Zmodem transfer in progress...]";
            return (ZmodemState::TransferStart, Bytes::from_static(msg));
        }

        if self.in_transfer {
            let msg = b"[Zmodem transfer in progress...]";
            return (ZmodemState::Transferring, Bytes::from_static(msg));
        }

        (ZmodemState::Normal, Bytes::copy_from_slice(data))
    }
}

impl Default for ZmodemMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionMiddleware for ZmodemMiddleware {
    fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes {
        let (_state, bytes) = self.detect_and_replace(data);
        bytes
    }

    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes {
        // User input is never modified.
        Bytes::copy_from_slice(data)
    }
}

/// Simple subsequence search – returns `true` if `needle` appears in `haystack`.
fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_data_passes_through() {
        let mut mw = ZmodemMiddleware::new();
        let result = mw.process_output_to_terminal(b"hello world");
        assert_eq!(&result[..], b"hello world");
    }

    #[test]
    fn detects_rz_start() {
        let mut mw = ZmodemMiddleware::new();
        let result = mw.process_output_to_terminal(b"rz\r");
        assert_eq!(&result[..], b"[Zmodem transfer in progress...]");
        assert!(mw.in_transfer);
    }

    #[test]
    fn detects_zrqinit_start() {
        let mut mw = ZmodemMiddleware::new();
        let result = mw.process_output_to_terminal(b"**\x18B0");
        assert_eq!(&result[..], b"[Zmodem transfer in progress...]");
    }

    #[test]
    fn replaces_binary_during_transfer() {
        let mut mw = ZmodemMiddleware::new();
        let _ = mw.process_output_to_terminal(b"rz\r");
        let result = mw.process_output_to_terminal(b"\x00\x01\x02binary junk");
        assert_eq!(&result[..], b"[Zmodem transfer in progress...]");
    }

    #[test]
    fn detects_end_and_resumes_normal() {
        let mut mw = ZmodemMiddleware::new();
        let _ = mw.process_output_to_terminal(b"rz\r");
        let _ = mw.process_output_to_terminal(b"**\x18B0OO");
        let result = mw.process_output_to_terminal(b"normal again");
        assert_eq!(&result[..], b"normal again");
    }

    #[test]
    fn session_side_is_passthrough() {
        let mut mw = ZmodemMiddleware::new();
        let result = mw.process_output_to_session(b"anything");
        assert_eq!(&result[..], b"anything");
    }
}
