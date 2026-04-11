use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use pureterm_session::Session;
use pureterm_terminal::Terminal;

/// Selection state within a terminal pane.
#[derive(Debug, Clone)]
pub struct Selection {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub is_block: bool,
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
    dirty: bool,
    output_buffer: Arc<Mutex<Vec<u8>>>,
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
            dirty: true,
            output_buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Set the session for this pane and start output processing.
    pub fn set_session(&mut self, session: Box<dyn Session>) {
        let receiver = session.output_stream();
        self.session = Some(session);
        self.start_output_processing(receiver);
    }

    /// Set the session with a pre-created receiver (avoids broadcast race condition).
    pub fn set_session_with_receiver(&mut self, session: Box<dyn Session>, receiver: broadcast::Receiver<Vec<u8>>) {
        self.session = Some(session);
        self.start_output_processing(receiver);
    }

    /// Start processing session output in a background thread.
    fn start_output_processing(&mut self, mut receiver: broadcast::Receiver<Vec<u8>>) {
        let output_buffer = Arc::clone(&self.output_buffer);

            std::thread::spawn(move || {
                loop {
                    match receiver.blocking_recv() {
                        Ok(data) => {
                            if let Ok(mut buffer) = output_buffer.lock() {
                                buffer.extend(data);
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
        let bytes = self.encode_key(key, ctrl, alt, shift);
        self.write_input(&bytes);
    }

    /// Encode a key event into terminal escape sequences.
    fn encode_key(&self, key: &str, ctrl: bool, alt: bool, shift: bool) -> Vec<u8> {
        log::debug!("encode_key: key={:?}, ctrl={}, alt={}, shift={}", key, ctrl, alt, shift);

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
                _ => {}
            }
        }

        // Handle special keys
        match key {
            "Enter" => return vec![0x0d],
            "Backspace" => return vec![0x7f],
            "Tab" => return vec![0x09],
            "Escape" => return vec![0x1b],
            "Up" => return b"\x1b[A".to_vec(),
            "Down" => return b"\x1b[B".to_vec(),
            "Right" => return b"\x1b[C".to_vec(),
            "Left" => return b"\x1b[D".to_vec(),
            "Home" => return b"\x1b[H".to_vec(),
            "End" => return b"\x1b[F".to_vec(),
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

        // Handle Shift + special keys
        if shift {
            match key {
                "Up" => return b"\x1b[1;2A".to_vec(),
                "Down" => return b"\x1b[1;2B".to_vec(),
                "Right" => return b"\x1b[1;2C".to_vec(),
                "Left" => return b"\x1b[1;2D".to_vec(),
                "Home" => return b"\x1b[1;2H".to_vec(),
                "End" => return b"\x1b[1;2F".to_vec(),
                "Tab" => return b"\x1b[Z".to_vec(),
                _ => {}
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

    /// Paste text into the terminal.
    pub fn paste(&mut self, text: &str) {
        // Handle bracketed paste mode
        // TODO: Check if bracketed paste mode is enabled
        self.write_input(text.as_bytes());
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
