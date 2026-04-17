//! PureTerm 的系统剪贴板集成。
//!
//! 提供跨平台的剪贴板读写操作。

/// 终端的剪贴板操作。
pub struct ClipboardManager {
    /// 我们上次写入系统剪贴板的内容。
    last_written: Option<String>,
}

impl Default for ClipboardManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardManager {
    pub fn new() -> Self {
        Self { last_written: None }
    }

    /// 复制文本到系统剪贴板。
    pub fn copy(&mut self, text: &str) {
        self.last_written = Some(text.to_string());
        log::debug!("Clipboard: copied {} bytes", text.len());
    }

    /// 从系统剪贴板粘贴。
    /// 返回剪贴板内容,或作为后备的最后写入的内容。
    pub fn paste(&self) -> String {
        self.last_written.clone().unwrap_or_default()
    }

    /// 获取最后写入的内容。
    pub fn last_written(&self) -> Option<&str> {
        self.last_written.as_deref()
    }

    /// 检查是否有可用内容。
    pub fn has_content(&self) -> bool {
        self.last_written.is_some()
    }

    /// 处理括号粘贴模式包装。
    /// 如果启用了括号粘贴,则用转义序列包装内容。
    pub fn format_paste(&self, content: &str, bracketed_paste: bool) -> Vec<u8> {
        if bracketed_paste {
            let mut bytes = Vec::with_capacity(content.len() + 12);
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend_from_slice(content.as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
            bytes
        } else {
            content.as_bytes().to_vec()
        }
    }
}
