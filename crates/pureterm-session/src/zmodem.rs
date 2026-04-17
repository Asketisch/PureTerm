use bytes::Bytes;

use crate::middleware::SessionMiddleware;

/// Zmodem 检测的当前状态。
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

/// 检测 Zmodem 文件传输序列的中间件,并将二进制数据替换为人类可读的占位符,
/// 以防止终端被原始字节淹没。
pub struct ZmodemMiddleware {
    in_transfer: bool,
}

/// Zmodem 起始标记: `**\x18B0`(ZRQINIT 头通常出现在 `rz\r` 之后)。
const ZMODEM_START: &[u8] = b"**\x18B0";

/// Zmodem 结束标记: `**\x18B0` 后跟 `OO`(over-and-out)。
const ZMODEM_END: &[u8] = b"**\x18B0OO";

impl ZmodemMiddleware {
    pub fn new() -> Self {
        Self {
            in_transfer: false,
        }
    }

    /// 在 `data` 中扫描 Zmodem 标记,并返回检测到的状态以及(可能被替换的)字节。
    fn detect_and_replace(&mut self, data: &[u8]) -> (ZmodemState, Bytes) {
        // 首先检查结束标记(它是起始标记的超集)。
        if self.in_transfer && contains_subsequence(data, ZMODEM_END) {
            self.in_transfer = false;
            let msg = b"[Zmodem transfer in progress...]";
            return (ZmodemState::TransferEnd, Bytes::from_static(msg));
        }

        // 检查起始标记。
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
        // 用户输入永远不会被修改。
        Bytes::copy_from_slice(data)
    }
}

/// 简单的子序列搜索 - 如果 `needle` 出现在 `haystack` 中则返回 `true`。
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
