//! 括号粘贴输入处理。

/// 如果终端启用了括号粘贴模式 (CSI ? 2004 h),则用括号粘贴转义序列包装粘贴内容。
pub struct BracketedPasteHandler;

impl BracketedPasteHandler {
    /// 为括号粘贴模式包装内容。
    /// 如果启用,返回用 ESC[200~ 和 ESC[201~ 包装的内容。
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

    /// 处理传入数据以检测和提取括号粘贴内容。
    /// 返回粘贴事件和原始输入段的列表。
    pub fn process_input(data: &[u8]) -> Vec<InputSegment> {
        let mut segments = Vec::new();
        let mut i = 0;
        let mut raw_start = 0;

        while i < data.len() {
            // 查找 ESC[200~ (粘贴开始)
            if i + 6 <= data.len()
                && data[i] == 0x1b
                && data[i + 1] == b'['
                && data[i + 2] == b'2'
                && data[i + 3] == b'0'
                && data[i + 4] == b'0'
                && data[i + 5] == b'~'
            {
                // 发出粘贴开始前的任何原始数据
                if i > raw_start {
                    segments.push(InputSegment::Raw(data[raw_start..i].to_vec()));
                }

                // 查找粘贴结束: ESC[201~
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
                    // 未找到结束标记 — 将剩余部分视为不完整的粘贴
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

/// 输入数据的一段,可以是原始按键或粘贴内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSegment {
    /// 原始键盘输入。
    Raw(Vec<u8>),
    /// 来自括号粘贴的内容。
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
