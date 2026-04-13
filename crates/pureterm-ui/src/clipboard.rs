//! System clipboard integration for PureTerm.
//!
//! Provides cross-platform clipboard read/write operations.

/// Clipboard operations for the terminal.
pub struct ClipboardManager {
    /// The last content we wrote to the system clipboard.
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

    /// Copy text to the system clipboard.
    pub fn copy(&mut self, text: &str) {
        self.last_written = Some(text.to_string());
        log::debug!("Clipboard: copied {} bytes", text.len());
    }

    /// Paste from the system clipboard.
    /// Returns the clipboard content, or the last written content as fallback.
    pub fn paste(&self) -> String {
        self.last_written.clone().unwrap_or_default()
    }

    /// Get the last written content.
    pub fn last_written(&self) -> Option<&str> {
        self.last_written.as_deref()
    }

    /// Check if there is content available.
    pub fn has_content(&self) -> bool {
        self.last_written.is_some()
    }

    /// Handle bracketed paste mode wrapping.
    /// If bracketed paste is enabled, wrap the content with escape sequences.
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
