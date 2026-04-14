use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::broadcast;

use pureterm_session::Session;
use pureterm_terminal::Terminal;
use pureterm_terminal::screen::selection::{
    self, ClickCount, MouseButton, MouseModifiers, TerminalMouseEvent,
};
use pureterm_terminal::mode::TermMode;

// ── Smooth Scroll State ───────────────────────────────────────────────────────

/// Smooth scrolling animation state
#[derive(Debug, Clone)]
pub struct ScrollState {
    /// Current animated scroll offset (can be fractional for smooth animation)
    pub current_offset: f32,
    /// Target scroll offset (where we want to scroll to)
    pub target_offset: f32,
    /// Velocity for momentum scrolling
    pub velocity: f32,
    /// Whether animation is in progress
    pub is_animating: bool,
    /// Animation start time
    pub animation_start: Option<Instant>,
    /// Animation duration
    pub animation_duration: Duration,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            current_offset: 0.0,
            target_offset: 0.0,
            velocity: 0.0,
            is_animating: false,
            animation_start: None,
            animation_duration: Duration::from_millis(150),
        }
    }
}

impl ScrollState {
    /// Create a new scroll state with default animation settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the target scroll offset and start animation if different from current
    pub fn set_target(&mut self, target: f32) {
        self.target_offset = target;
        if (self.target_offset - self.current_offset).abs() > 0.01 {
            self.is_animating = true;
            self.animation_start = Some(Instant::now());
        }
    }

    /// Update the scroll state, returns the new interpolated offset
    pub fn update(&mut self) -> f32 {
        if !self.is_animating {
            return self.current_offset;
        }

        let start = self.animation_start.unwrap_or(Instant::now());
        let elapsed = start.elapsed();
        let t = (elapsed.as_millis() as f32 / self.animation_duration.as_millis() as f32).min(1.0);

        // Ease-out cubic: t * (2 - t)
        let eased = t * (2.0 - t);

        let start_offset = self.current_offset;
        let diff = self.target_offset - start_offset;
        self.current_offset = start_offset + diff * eased;

        if t >= 1.0 {
            self.current_offset = self.target_offset;
            self.is_animating = false;
        }

        self.current_offset
    }

    /// Set scroll offset immediately (no animation)
    pub fn set_immediate(&mut self, offset: f32) {
        self.current_offset = offset;
        self.target_offset = offset;
        self.is_animating = false;
        self.velocity = 0.0;
    }

    /// Add velocity for momentum scrolling (e.g., from mouse wheel)
    pub fn add_velocity(&mut self, delta: f32) {
        self.velocity += delta;
        self.is_animating = true;
        self.animation_start = Some(Instant::now());
    }

    /// Update with momentum, gradually decelerating
    pub fn update_momentum(&mut self) -> f32 {
        if self.velocity.abs() < 0.01 {
            self.velocity = 0.0;
            self.is_animating = false;
            return self.current_offset;
        }

        // Apply friction
        self.velocity *= 0.95;
        self.current_offset += self.velocity;
        self.target_offset = self.current_offset;

        self.is_animating = true;
        self.current_offset
    }
}

/// Selection state within a terminal pane.
#[derive(Debug, Clone)]
pub struct Selection {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub is_block: bool,
}

impl Selection {
    /// Check if a cell at (row, col) is within the selection.
    pub fn contains(&self, row: usize, col: usize) -> bool {
        if self.is_block {
            let min_col = self.start_col.min(self.end_col);
            let max_col = self.start_col.max(self.end_col);
            let min_row = self.start_row.min(self.end_row);
            let max_row = self.start_row.max(self.end_row);
            row >= min_row && row <= max_row && col >= min_col && col <= max_col
        } else {
            // Normal selection: start and end can be in any order
            let (sr, sc) = (self.start_row, self.start_col);
            let (er, ec) = (self.end_row, self.end_col);
            if sr == er {
                row == sr && col >= sc.min(ec) && col <= sc.max(ec)
            } else if sr < er {
                (row == sr && col >= sc)
                    || (row > sr && row < er)
                    || (row == er && col <= ec)
            } else {
                (row == er && col >= ec)
                    || (row > er && row < sr)
                    || (row == sr && col <= sc)
            }
        }
    }
}

/// Search state.
#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: String,
    pub matches: Vec<(usize, usize, usize)>, // (row, start_col, end_col)
    pub current_match: Option<usize>,
    pub case_sensitive: bool,
    pub regex: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            current_match: None,
            case_sensitive: false,
            regex: false,
        }
    }
}

/// Internal mouse state tracking.
struct MouseState {
    /// Whether a mouse button is currently held down (for drag selection).
    left_down: bool,
    /// Start position of the current drag.
    drag_start: Option<(usize, usize)>,
    /// Time of last click for double/triple click detection.
    last_click_time: Option<Instant>,
    /// Position of last click.
    last_click_pos: Option<(usize, usize)>,
    /// Click count (1=single, 2=double, 3=triple).
    click_count: u8,
}

impl Default for MouseState {
    fn default() -> Self {
        Self {
            left_down: false,
            drag_start: None,
            last_click_time: None,
            last_click_pos: None,
            click_count: 0,
        }
    }
}

/// A terminal pane that combines a Terminal with a Session.
pub struct TerminalPane {
    pub id: u64,
    pub terminal: Terminal,
    pub session: Option<Box<dyn Session>>,
    pub selection: Option<Selection>,
    pub search: SearchState,
    pub zoom: f32,
    pub title: String,
    pub has_focus: bool,
    pub has_activity: bool,
    pub visual_bell_active: bool,
    dirty: bool,
    output_buffer: Arc<Mutex<Vec<u8>>>,
    mouse_state: MouseState,
    /// Pending files dropped onto the terminal.
    pub dropped_files: Vec<String>,
    /// Whether copy-on-select is enabled.
    pub copy_on_select: bool,
    /// Callback for clipboard copy (set by the application).
    pub on_copy: Option<Box<dyn FnMut(&str) + Send>>,
    /// Smooth scroll state for animated scrolling
    pub scroll_state: ScrollState,
}

static NEXT_PANE_ID: AtomicU64 = AtomicU64::new(1);

fn next_pane_id() -> u64 {
    NEXT_PANE_ID.fetch_add(1, Ordering::SeqCst)
}

impl TerminalPane {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        Self {
            id: next_pane_id(),
            terminal: Terminal::new(cols, rows, scrollback),
            session: None,
            selection: None,
            search: SearchState::default(),
            zoom: 1.0,
            title: String::from("Terminal"),
            has_focus: false,
            has_activity: false,
            visual_bell_active: false,
            dirty: true,
            output_buffer: Arc::new(Mutex::new(Vec::new())),
            mouse_state: MouseState::default(),
            dropped_files: Vec::new(),
            copy_on_select: true,
            on_copy: None,
            scroll_state: ScrollState::new(),
        }
    }

    /// Set the session for this pane and start output processing.
    pub fn set_session(&mut self, session: Box<dyn Session>, waker: Option<Arc<dyn Fn() + Send + Sync>>) {
        let receiver = session.output_stream();
        self.session = Some(session);
        self.start_output_processing(receiver, waker);
    }

    /// Set the session with a pre-created receiver (avoids broadcast race condition).
    pub fn set_session_with_receiver(&mut self, session: Box<dyn Session>, receiver: broadcast::Receiver<Vec<u8>>, waker: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.session = Some(session);
        self.start_output_processing(receiver, waker);
    }

    /// Start processing session output in a background thread.
    fn start_output_processing(&mut self, mut receiver: broadcast::Receiver<Vec<u8>>, waker: Option<Arc<dyn Fn() + Send + Sync>>) {
        let output_buffer = Arc::clone(&self.output_buffer);

            std::thread::spawn(move || {
                loop {
                    match receiver.blocking_recv() {
                        Ok(data) => {
                            if let Ok(mut buffer) = output_buffer.lock() {
                                buffer.extend(data);
                            }
                            if let Some(ref waker) = waker {
                                waker();
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("Output lagged by {} messages", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
            });
    }

    /// Drain pending output from the session and process it.
    pub fn drain_output(&mut self) {
        if let Ok(mut buffer) = self.output_buffer.lock() {
            if !buffer.is_empty() {
                log::debug!("drain_output: {} bytes from session", buffer.len());
                self.terminal.write(&buffer);
                self.dirty = true;

                // Scroll to bottom on new output if user is at the bottom
                if self.terminal.scroll_offset() == 0 {
                    // Already at bottom — stay at bottom
                }
                // If user scrolled up, don't force scroll down
                buffer.clear();
            }
        }
    }

    /// Write input data to the session.
    pub fn write_input(&mut self, data: &[u8]) {
        if let Some(ref mut session) = self.session {
            log::debug!("write_input: {} bytes = {:02x?}", data.len(), data);
            if let Err(e) = session.write(data) {
                log::error!("Failed to write to session: {}", e);
            }
        } else {
            log::debug!("write_input: no session, {} bytes dropped", data.len());
        }
    }

    /// Process output from the session.
    pub fn process_output(&mut self, data: &[u8]) {
        self.terminal.write(data);
        self.dirty = true;
    }

    /// Handle a key press for terminal input.
    pub fn handle_key(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool) {
        // Auto-scroll to bottom on input
        if self.terminal.scroll_offset() > 0 {
            self.terminal.scroll_to_bottom();
            self.dirty = true;
        }
        let bytes = self.encode_key(key, ctrl, alt, shift);
        self.write_input(&bytes);
    }

    /// Get the smooth scroll visual offset (fractional pixels for smooth scrolling)
    pub fn scroll_visual_offset(&self) -> f32 {
        self.scroll_state.current_offset
    }

    /// Update the scroll animation. Call this every frame.
    /// Returns the updated visual offset.
    pub fn update_scroll_animation(&mut self) -> f32 {
        if self.scroll_state.is_animating {
            self.dirty = true;
        }
        self.scroll_state.update()
    }

    /// Get whether smooth scroll animation is in progress
    pub fn is_scrolling_animated(&self) -> bool {
        self.scroll_state.is_animating
    }

    /// Encode a key event into terminal escape sequences.
    fn encode_key(&self, key: &str, ctrl: bool, alt: bool, shift: bool) -> Vec<u8> {
        log::debug!("encode_key: key={:?}, ctrl={}, alt={}, shift={}", key, ctrl, alt, shift);

        let app_cursor = self.terminal.mode().contains(TermMode::APP_CURSOR);

        // Handle control characters
        if ctrl && !alt && !shift {
            match key {
                "a" => return vec![0x01],
                "b" => return vec![0x02],
                "c" => return vec![0x03],
                "d" => return vec![0x04],
                "e" => return vec![0x05],
                "f" => return vec![0x06],
                "g" => return vec![0x07],
                "h" => return vec![0x08],
                "i" => return vec![0x09],
                "j" => return vec![0x0a],
                "k" => return vec![0x0b],
                "l" => return vec![0x0c],
                "m" => return vec![0x0d],
                "n" => return vec![0x0e],
                "o" => return vec![0x0f],
                "p" => return vec![0x10],
                "q" => return vec![0x11],
                "r" => return vec![0x12],
                "s" => return vec![0x13],
                "t" => return vec![0x14],
                "u" => return vec![0x15],
                "v" => return vec![0x16],
                "w" => return vec![0x17],
                "x" => return vec![0x18],
                "y" => return vec![0x19],
                "z" => return vec![0x1a],
                "[" => return vec![0x1b],
                "\\" => return vec![0x1c],
                "]" => return vec![0x1d],
                "/" => return vec![0x1f],
                _ => {}
            }
        }

        // Handle Ctrl + special keys (word navigation, etc.)
        if ctrl && !alt {
            match key {
                "Up" => return b"\x1b[1;5A".to_vec(),
                "Down" => return b"\x1b[1;5B".to_vec(),
                "Right" => return b"\x1b[1;5C".to_vec(),
                "Left" => return b"\x1b[1;5D".to_vec(),
                "Home" => return b"\x1b[1;5H".to_vec(),
                "End" => return b"\x1b[1;5F".to_vec(),
                _ => {}
            }
        }

        // Handle Alt + special keys
        if alt && !ctrl {
            match key {
                "Up" => return b"\x1b[1;3A".to_vec(),
                "Down" => return b"\x1b[1;3B".to_vec(),
                "Right" => return b"\x1b[1;3C".to_vec(),
                "Left" => return b"\x1b[1;3D".to_vec(),
                "Home" => return b"\x1b[1;3H".to_vec(),
                "End" => return b"\x1b[1;3F".to_vec(),
                _ => {}
            }
        }

        // Handle Shift + Ctrl + special keys
        if ctrl && shift && !alt {
            match key {
                "Up" => return b"\x1b[1;6A".to_vec(),
                "Down" => return b"\x1b[1;6B".to_vec(),
                "Right" => return b"\x1b[1;6C".to_vec(),
                "Left" => return b"\x1b[1;6D".to_vec(),
                _ => {}
            }
        }

        // Handle special keys (with app cursor mode support)
        match key {
            "Enter" => return vec![0x0d],
            "Backspace" => return vec![0x7f],
            "Tab" => return vec![0x09],
            "Escape" => return vec![0x1b],
            "Up" => {
                return if app_cursor {
                    b"\x1bOA".to_vec()
                } else {
                    b"\x1b[A".to_vec()
                };
            }
            "Down" => {
                return if app_cursor {
                    b"\x1bOB".to_vec()
                } else {
                    b"\x1b[B".to_vec()
                };
            }
            "Right" => {
                return if app_cursor {
                    b"\x1bOC".to_vec()
                } else {
                    b"\x1b[C".to_vec()
                };
            }
            "Left" => {
                return if app_cursor {
                    b"\x1bOD".to_vec()
                } else {
                    b"\x1b[D".to_vec()
                };
            }
            "Home" => {
                return if app_cursor {
                    b"\x1bOH".to_vec()
                } else {
                    b"\x1b[H".to_vec()
                };
            }
            "End" => {
                return if app_cursor {
                    b"\x1bOF".to_vec()
                } else {
                    b"\x1b[F".to_vec()
                };
            }
            "PageUp" => return b"\x1b[5~".to_vec(),
            "PageDown" => return b"\x1b[6~".to_vec(),
            "Insert" => return b"\x1b[2~".to_vec(),
            "Delete" => return b"\x1b[3~".to_vec(),
            "F1" => return b"\x1bOP".to_vec(),
            "F2" => return b"\x1bOQ".to_vec(),
            "F3" => return b"\x1bOR".to_vec(),
            "F4" => return b"\x1bOS".to_vec(),
            "F5" => return b"\x1b[15~".to_vec(),
            "F6" => return b"\x1b[17~".to_vec(),
            "F7" => return b"\x1b[18~".to_vec(),
            "F8" => return b"\x1b[19~".to_vec(),
            "F9" => return b"\x1b[20~".to_vec(),
            "F10" => return b"\x1b[21~".to_vec(),
            "F11" => return b"\x1b[23~".to_vec(),
            "F12" => return b"\x1b[24~".to_vec(),
            _ => {}
        }

        // Handle Shift + special keys (fallback for non-arrow keys)
        if shift {
            match key {
                "Tab" => return b"\x1b[Z".to_vec(),
                _ => {}
            }
        }

        // Handle Alt modifier for printable characters
        if alt && !ctrl {
            let bytes = key.as_bytes();
            if !bytes.is_empty() && bytes[0] >= 0x20 && bytes[0] < 0x7f {
                let mut result = vec![0x1b];
                result.extend_from_slice(bytes);
                return result;
            }
        }

        // Regular character input
        key.as_bytes().to_vec()
    }

    /// Resize the terminal and PTY.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.terminal.resize(cols, rows);
        if let Some(ref mut session) = self.session {
            if let Err(e) = session.resize(cols as u16, rows as u16) {
                log::error!("Failed to resize session: {}", e);
            }
        }
        self.dirty = true;
    }

    /// Copy the selected text.
    pub fn copy_selection(&self) -> Option<String> {
        self.selection.as_ref().map(|sel| {
            self.terminal.get_text(
                sel.start_row, sel.start_col,
                sel.end_row, sel.end_col,
            )
        })
    }

    /// Paste text into the terminal, respecting bracketed paste mode.
    pub fn paste(&mut self, text: &str) {
        // Trim trailing newlines to prevent accidental command execution
        let trimmed = text.trim_end_matches(|c| c == '\n' || c == '\r');
        if trimmed.is_empty() {
            return;
        }
        let bracketed = self.terminal.mode().contains(TermMode::BRACKETED_PASTE);
        if bracketed {
            let mut data = Vec::with_capacity(trimmed.len() + 12);
            data.extend_from_slice(b"\x1b[200~");
            data.extend_from_slice(trimmed.as_bytes());
            data.extend_from_slice(b"\x1b[201~");
            self.write_input(&data);
        } else {
            self.write_input(trimmed.as_bytes());
        }
    }

    /// Zoom in.
    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom + 0.1).min(3.0);
        self.dirty = true;
    }

    /// Zoom out.
    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom - 0.1).max(0.5);
        self.dirty = true;
    }

    /// Reset zoom.
    pub fn zoom_reset(&mut self) {
        self.zoom = 1.0;
        self.dirty = true;
    }

    /// Check if the pane is dirty and needs re-rendering.
    pub fn is_dirty(&self) -> bool {
        self.dirty || self.terminal.is_dirty()
    }

    /// Clear the dirty flag.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
        self.terminal.clear_dirty();
    }

    /// Mark the pane as dirty so it will be re-rendered.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    // ── Mouse event handling ──

    /// Convert pixel coordinates to terminal grid (row, col).
    pub fn pixel_to_cell(&self, px: f32, py: f32, cell_w: f32, cell_h: f32) -> (usize, usize) {
        let col = (px / cell_w).floor() as usize;
        let row = (py / cell_h).floor() as usize;
        let col = col.min(self.terminal.cols().saturating_sub(1));
        let row = row.min(self.terminal.rows().saturating_sub(1));
        (row, col)
    }

    /// Handle a mouse event.
    /// Returns bytes to send to the child process (for mouse mode) and any action to take.
    pub fn handle_mouse_event(
        &mut self,
        event: TerminalMouseEvent,
    ) -> (Option<Vec<u8>>, Option<PaneAction>) {
        let mode = self.terminal.mode();
        let mouse_active = mode.contains(TermMode::MOUSE_REPORT_CLICK)
            || mode.contains(TermMode::MOUSE_DRAG)
            || mode.contains(TermMode::MOUSE_MOTION);

        match event {
            TerminalMouseEvent::Down { button, row, col, modifiers } => {
                if mouse_active && button == MouseButton::Left && !modifiers.shift {
                    // Forward to application
                    let bytes = self.encode_mouse_event(button, true, row, col, modifiers);
                    return (Some(bytes), None);
                }

                match button {
                    MouseButton::Left => {
                        return (None, self.handle_left_click(row, col, modifiers));
                    }
                    MouseButton::Middle => {
                        // Middle-click paste
                        return (None, Some(PaneAction::PasteFromClipboard));
                    }
                    MouseButton::Right => {
                        // Right-click: copy selection if exists, then show context menu
                        if let Some(sel) = &self.selection {
                            let text = self.terminal.get_text(
                                sel.start_row, sel.start_col,
                                sel.end_row, sel.end_col,
                            );
                            if !text.is_empty() {
                                return (None, Some(PaneAction::CopyText(text)));
                            }
                        }
                        return (None, Some(PaneAction::ShowContextMenu));
                    }
                }
            }
            TerminalMouseEvent::Up { button, row, col, modifiers } => {
                if mouse_active && button == MouseButton::Left && !modifiers.shift {
                    let bytes = self.encode_mouse_event(button, false, row, col, modifiers);
                    return (Some(bytes), None);
                }

                if button == MouseButton::Left && self.mouse_state.left_down {
                    self.mouse_state.left_down = false;
                    // Finalize selection
                    if let Some(ref sel) = self.selection {
                        let text = self.terminal.get_text(
                            sel.start_row, sel.start_col,
                            sel.end_row, sel.end_col,
                        );
                        if !text.trim().is_empty() && self.copy_on_select {
                            return (None, Some(PaneAction::CopyText(text)));
                        }
                    }
                }
                (None, None)
            }
            TerminalMouseEvent::Move { row, col, modifiers } => {
                if mouse_active && self.mouse_state.left_down && !modifiers.shift {
                    let report_drag = mode.contains(TermMode::MOUSE_DRAG)
                        || mode.contains(TermMode::MOUSE_MOTION);
                    if report_drag {
                        let bytes = self.encode_mouse_event(MouseButton::Left, false, row, col, modifiers);
                        return (Some(bytes), None);
                    }
                }

                // Extend selection during drag
                if self.mouse_state.left_down {
                    if let Some(ref mut sel) = self.selection {
                        sel.end_row = row;
                        sel.end_col = col;
                        self.dirty = true;
                    }
                }
                (None, None)
            }
            TerminalMouseEvent::Scroll { delta_rows, row, col, modifiers } => {
                if mouse_active {
                    // Mouse wheel events are encoded as button 64 (up) / 65 (down)
                    let code = if delta_rows > 0 { 64 } else { 65 };
                    if mode.contains(TermMode::MOUSE_SGR) {
                        let bytes = format!("\x1b[<{};{};{}M", code, col + 1, row + 1).into_bytes();
                        return (Some(bytes), None);
                    } else {
                        // X10-style encoding for scroll
                        let bytes = selection::encode_mouse_x10(
                            if delta_rows > 0 { MouseButton::Middle } else { MouseButton::Middle },
                            true, row, col, modifiers,
                        );
                        return (Some(bytes), None);
                    }
                }

                // Alt + scroll sends arrow keys (useful for less, etc.)
                if modifiers.alt {
                    let bytes = if delta_rows > 0 {
                        b"\x1b[A".to_vec()
                    } else {
                        b"\x1b[B".to_vec()
                    };
                    return (Some(bytes), None);
                }

                // Normal scroll with smooth scrolling animation
                if delta_rows > 0 {
                    let current = self.scroll_state.current_offset;
                    self.scroll_state.set_target(current + delta_rows as f32);
                    self.terminal.scroll_up(delta_rows as usize);
                } else {
                    let current = self.scroll_state.current_offset;
                    self.scroll_state.set_target(current - (-delta_rows) as f32);
                    self.terminal.scroll_down((-delta_rows) as usize);
                }
                self.dirty = true;
                (None, None)
            }
        }
    }

    fn handle_left_click(&mut self, row: usize, col: usize, modifiers: MouseModifiers) -> Option<PaneAction> {
        let now = Instant::now();
        let click_count = if let Some(last_time) = self.mouse_state.last_click_time {
            if now.duration_since(last_time) < std::time::Duration::from_millis(500) {
                if let Some((lr, lc)) = self.mouse_state.last_click_pos {
                    if lr == row && lc == col {
                        self.mouse_state.click_count + 1
                    } else {
                        1
                    }
                } else {
                    1
                }
            } else {
                1
            }
        } else {
            1
        };

        self.mouse_state.click_count = click_count;
        self.mouse_state.last_click_time = Some(now);
        self.mouse_state.last_click_pos = Some((row, col));
        self.mouse_state.left_down = true;
        self.mouse_state.drag_start = Some((row, col));

        match ClickCount::from_count(click_count) {
            ClickCount::Single => {
                // Start new selection (or clear if no shift)
                if !modifiers.shift {
                    self.selection = None;
                }
                // Place selection start at click position
                if modifiers.shift {
                    // Extend existing selection
                    if let Some(ref mut sel) = self.selection {
                        sel.end_row = row;
                        sel.end_col = col;
                    } else {
                        self.selection = Some(Selection {
                            start_row: row,
                            start_col: col,
                            end_row: row,
                            end_col: col,
                            is_block: modifiers.alt,
                        });
                    }
                } else {
                    self.selection = Some(Selection {
                        start_row: row,
                        start_col: col,
                        end_row: row,
                        end_col: col,
                        is_block: modifiers.alt,
                    });
                }
            }
            ClickCount::Double => {
                // Select word at position
                let line_chars: Vec<char> = if let Some(row_cells) = self.terminal.snapshot().rows.get(row) {
                    row_cells.iter().map(|c| c.c).collect()
                } else {
                    vec![]
                };
                let (start, end) = selection::find_word_bounds(&line_chars, col);
                self.selection = Some(Selection {
                    start_row: row,
                    start_col: start,
                    end_row: row,
                    end_col: end,
                    is_block: false,
                });
                let text = self.terminal.get_text(row, start, row, end);
                if !text.trim().is_empty() && self.copy_on_select {
                    return Some(PaneAction::CopyText(text));
                }
            }
            ClickCount::Triple => {
                // Select entire line
                let line_end = self.terminal.cols().saturating_sub(1);
                self.selection = Some(Selection {
                    start_row: row,
                    start_col: 0,
                    end_row: row,
                    end_col: line_end,
                    is_block: false,
                });
                let text = self.terminal.get_text(row, 0, row, line_end);
                if !text.trim().is_empty() && self.copy_on_select {
                    return Some(PaneAction::CopyText(text));
                }
            }
        }

        self.dirty = true;
        None
    }

    /// Encode a mouse event for forwarding to the child process.
    fn encode_mouse_event(&self, button: MouseButton, pressed: bool, row: usize, col: usize, modifiers: MouseModifiers) -> Vec<u8> {
        let mode = self.terminal.mode();
        if mode.contains(TermMode::MOUSE_SGR) {
            selection::encode_mouse_sgr(button, pressed, row, col, modifiers)
        } else {
            selection::encode_mouse_x10(button, pressed, row, col, modifiers)
        }
    }

    /// Handle a dropped file event.
    pub fn handle_dropped_file(&mut self, path: &str) {
        // Quote the path and send it as input
        let escaped = if path.contains(' ') || path.contains('\'') || path.contains('"') {
            format!("'{}'", path.replace("'", "'\\''"))
        } else {
            path.to_string()
        };
        self.write_input(escaped.as_bytes());
        self.write_input(b" ");
    }

    /// Handle dropped files.
    pub fn handle_dropped_files(&mut self, paths: Vec<String>) {
        for path in &paths {
            self.handle_dropped_file(path);
        }
    }

    /// Get the hyperlink at a given cell position, if any.
    pub fn hyperlink_at(&self, row: usize, col: usize) -> Option<std::sync::Arc<pureterm_terminal::Hyperlink>> {
        let snapshot = self.terminal.snapshot();
        snapshot.rows.get(row)
            .and_then(|r| r.get(col))
            .and_then(|cell| cell.hyperlink.clone())
    }

    /// Check if the cell at (row, col) contains a URL-like text (heuristic detection).
    pub fn url_at(&self, row: usize, col: usize) -> Option<String> {
        // First check OSC 8 hyperlink
        if let Some(link) = self.hyperlink_at(row, col) {
            return Some(link.uri.clone());
        }

        // Then try URL detection in the text
        let snapshot = self.terminal.snapshot();
        let row_cells = snapshot.rows.get(row)?;
        let line: String = row_cells.iter().map(|c| c.c).collect();

        detect_url_in_line(&line, col)
    }

    /// Update the search state.
    pub fn update_search(&mut self) {
        if self.search.query.is_empty() {
            self.search.matches.clear();
            self.search.current_match = None;
            return;
        }

        self.search.matches.clear();
        let visible_text = self.terminal.get_visible_text();

        for (row, line) in visible_text.lines().enumerate() {
            let mut start = 0;
            while let Some(pos) = line[self.search.query.len() + start..].find(&self.search.query) {
                let col = start + pos;
                self.search.matches.push((row, col, col + self.search.query.len()));
                start = col + 1;
            }
        }

        self.search.current_match = if self.search.matches.is_empty() {
            None
        } else {
            Some(0)
        };
    }
}

/// Detect a URL in a line of text that includes the given column position.
fn detect_url_in_line(line: &str, col: usize) -> Option<String> {
    // Simple URL detection: find http/https/ftp/file/www patterns
    let url_pattern = regex::Regex::new(
        r"(?:https?://|ftp://|file://|www\.)\S+"
    ).ok()?;

    for m in url_pattern.find_iter(line) {
        if col >= m.start() && col <= m.end() {
            let mut url = m.as_str().to_string();
            // Clean trailing punctuation
            while url.ends_with(&['.', ',', ';', ':', '!', '?', ')', ']', '}'][..]) {
                url.pop();
            }
            if !url.starts_with("http") && !url.starts_with("ftp") && !url.starts_with("file") {
                url = format!("https://{}", url);
            }
            return Some(url);
        }
    }
    None
}

/// Actions that the pane requests from the application.
#[derive(Debug, Clone)]
pub enum PaneAction {
    /// Copy the given text to the clipboard.
    CopyText(String),
    /// Paste from the system clipboard.
    PasteFromClipboard,
    /// Show the context menu.
    ShowContextMenu,
    /// Open a URL in the default browser.
    OpenUrl(String),
    /// Split the pane to the right.
    SplitRight,
    /// Split the pane to the left.
    SplitLeft,
    /// Split the pane downward.
    SplitDown,
    /// Split the pane upward.
    SplitUp,
    /// Edit the tab title.
    EditTabTitle,
    /// Edit the tab color.
    EditTabColor,
}
