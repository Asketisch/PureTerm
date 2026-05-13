//! macOS VoiceOver accessibility support.
//!
//! Exposes terminal content via NSAccessibility protocol so that
//! VoiceOver can read terminal text, announce cursor position,
//! and navigate the terminal content.

/// Text content snapshot for accessibility.
#[derive(Debug, Clone)]
pub struct AccessibilitySnapshot {
    /// Visible terminal lines (row 0 = top of viewport)
    pub lines: Vec<String>,
    /// Cursor row (0-based, relative to viewport)
    pub cursor_row: usize,
    /// Cursor column (0-based)
    pub cursor_col: usize,
    /// Terminal width in columns
    pub width: usize,
    /// Terminal height in rows
    pub height: usize,
}

impl AccessibilitySnapshot {
    /// Get the line at the cursor position.
    pub fn cursor_line(&self) -> Option<&str> {
        self.lines.get(self.cursor_row).map(|s| s.as_str())
    }

    /// Get the full terminal content as a single string.
    pub fn full_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Get the word at the cursor position.
    pub fn word_at_cursor(&self) -> Option<String> {
        let line = self.lines.get(self.cursor_row)?;
        if self.cursor_col >= line.len() {
            return None;
        }
        let chars: Vec<char> = line.chars().collect();
        let col = self.cursor_col.min(chars.len().saturating_sub(1));
        if chars[col].is_whitespace() {
            return None;
        }
        let start = (0..col).rev().find(|&i| chars[i].is_whitespace()).map(|i| i + 1).unwrap_or(0);
        let end = (col + 1..chars.len()).find(|&i| chars[i].is_whitespace()).unwrap_or(chars.len());
        Some(chars[start..end].iter().collect())
    }

    /// Get the line number announcement (1-based for users).
    pub fn cursor_line_number(&self) -> usize {
        self.cursor_row + 1
    }

    /// Get the column number announcement (1-based for users).
    pub fn cursor_column_number(&self) -> usize {
        self.cursor_col + 1
    }
}

/// Accessibility announcer for terminal events.
#[derive(Debug, Clone)]
pub struct AccessibilityAnnouncer {
    /// Whether VoiceOver appears to be active
    enabled: bool,
}

impl AccessibilityAnnouncer {
    pub fn new() -> Self {
        Self { enabled: false }
    }

    /// Check if accessibility is likely needed.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable/disable accessibility announcements.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Build an announcement string for the current cursor position.
    pub fn announce_cursor(&self, snapshot: &AccessibilitySnapshot) -> String {
        if !self.enabled {
            return String::new();
        }
        let line_num = snapshot.cursor_line_number();
        let col_num = snapshot.cursor_column_number();
        let current_char = snapshot
            .lines
            .get(snapshot.cursor_row)
            .and_then(|line| line.chars().nth(snapshot.cursor_col))
            .unwrap_or(' ');

        if current_char.is_whitespace() || current_char == '\0' {
            format!("line {}, column {}", line_num, col_num)
        } else {
            format!("line {}, column {}: {}", line_num, col_num, current_char)
        }
    }

    /// Build an announcement for the current line.
    pub fn announce_line(&self, snapshot: &AccessibilitySnapshot) -> String {
        if !self.enabled {
            return String::new();
        }
        let line_num = snapshot.cursor_line_number();
        match snapshot.lines.get(snapshot.cursor_row) {
            Some(line) if !line.trim().is_empty() => {
                format!("line {}: {}", line_num, line.trim_end())
            }
            _ => format!("line {}: blank", line_num),
        }
    }

    /// Build an announcement for a selection.
    pub fn announce_selection(&self, text: &str) -> String {
        if !self.enabled {
            return String::new();
        }
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return "selection empty".to_string();
        }
        let char_count = trimmed.chars().count();
        if char_count <= 100 {
            format!("selected: {}", trimmed)
        } else {
            let preview: String = trimmed.chars().take(100).collect();
            format!("selected {} characters: {}...", char_count, preview)
        }
    }
}

impl Default for AccessibilityAnnouncer {
    fn default() -> Self {
        Self::new()
    }
}

/// macOS-specific accessibility setup.
#[cfg(target_os = "macos")]
pub mod platform {
    use objc2::runtime::AnyObject;

    /// Set accessibility attributes on the window's content view.
    ///
    /// This makes VoiceOver treat the terminal as a text area.
    pub fn setup_accessibility(window: &winit::window::Window) {
        use raw_window_handle::HasWindowHandle;
        use raw_window_handle::RawWindowHandle;

        let handle = match window.window_handle() {
            Ok(h) => h,
            Err(_) => return,
        };

        let appkit_handle = match handle.as_raw() {
            RawWindowHandle::AppKit(h) => h,
            _ => return,
        };

        let ns_view_ptr = appkit_handle.ns_view.as_ptr();
        if ns_view_ptr.is_null() {
            return;
        }

        unsafe {
            let ns_view: &AnyObject = &*(ns_view_ptr as *const AnyObject);

            // Mark as accessibility element
            let _: () = objc2::msg_send![ns_view, setAccessibilityElement: true];

            // Set role to AXTextArea (terminal text content)
            let role = objc2_foundation::NSString::from_str("AXTextArea");
            let _: () = objc2::msg_send![ns_view, setAccessibilityRole: &*role];

            // Set subrole for terminal
            let subrole = objc2_foundation::NSString::from_str("AXTerminal");
            let _: () = objc2::msg_send![ns_view, setAccessibilitySubrole: &*subrole];

            // Set label
            let label = objc2_foundation::NSString::from_str("Terminal Content");
            let _: () = objc2::msg_send![ns_view, setAccessibilityLabel: &*label];

            // Set as focusable
            let _: () = objc2::msg_send![ns_view, setAccessibilityFocused: true];

            log::info!("macos: accessibility attributes set on terminal view");
        }
    }

    /// Update the accessibility value with the current terminal text.
    pub fn update_accessibility_value(window: &winit::window::Window, text: &str) {
        use raw_window_handle::HasWindowHandle;
        use raw_window_handle::RawWindowHandle;

        let handle = match window.window_handle() {
            Ok(h) => h,
            Err(_) => return,
        };

        let appkit_handle = match handle.as_raw() {
            RawWindowHandle::AppKit(h) => h,
            _ => return,
        };

        let ns_view_ptr = appkit_handle.ns_view.as_ptr();
        if ns_view_ptr.is_null() {
            return;
        }

        unsafe {
            let ns_view: &AnyObject = &*(ns_view_ptr as *const AnyObject);

            // Update the accessibility value with current terminal content
            let value = objc2_foundation::NSString::from_str(text);
            let _: () = objc2::msg_send![ns_view, setAccessibilityValue: &*value];
        }
    }

    /// Post a VoiceOver announcement.
    pub fn announce(text: &str) {
        unsafe {
            let ns_workspace: *mut AnyObject = objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
            if ns_workspace.is_null() {
                return;
            }

            let announcement = objc2_foundation::NSString::from_str(text);
            let _: () = objc2::msg_send![
                ns_workspace,
                accessibilityAnnounce: &*announcement
            ];
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub mod platform {
    pub fn setup_accessibility(_window: &winit::window::Window) {}
    pub fn update_accessibility_value(_window: &winit::window::Window, _text: &str) {}
    pub fn announce(_text: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_word_at_cursor() {
        let snapshot = AccessibilitySnapshot {
            lines: vec!["hello world".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            width: 80,
            height: 24,
        };
        assert_eq!(snapshot.word_at_cursor(), Some("hello".to_string()));

        let snapshot2 = AccessibilitySnapshot {
            lines: vec!["hello world".to_string()],
            cursor_row: 0,
            cursor_col: 6,
            width: 80,
            height: 24,
        };
        assert_eq!(snapshot2.word_at_cursor(), Some("world".to_string()));
    }

    #[test]
    fn test_snapshot_full_text() {
        let snapshot = AccessibilitySnapshot {
            lines: vec!["line1".to_string(), "line2".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            width: 80,
            height: 24,
        };
        assert_eq!(snapshot.full_text(), "line1\nline2");
    }

    #[test]
    fn test_announcer_cursor() {
        let mut announcer = AccessibilityAnnouncer::new();
        announcer.set_enabled(true);

        let snapshot = AccessibilitySnapshot {
            lines: vec!["hello".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            width: 80,
            height: 24,
        };
        let msg = announcer.announce_cursor(&snapshot);
        assert_eq!(msg, "line 1, column 1: h");
    }

    #[test]
    fn test_announcer_disabled() {
        let announcer = AccessibilityAnnouncer::new();
        assert!(!announcer.is_enabled());

        let snapshot = AccessibilitySnapshot {
            lines: vec!["hello".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            width: 80,
            height: 24,
        };
        assert!(announcer.announce_cursor(&snapshot).is_empty());
    }

    #[test]
    fn test_announcer_selection() {
        let mut announcer = AccessibilityAnnouncer::new();
        announcer.set_enabled(true);
        assert_eq!(announcer.announce_selection("hello world"), "selected: hello world");
        assert_eq!(announcer.announce_selection(""), "selection empty");
    }
}
