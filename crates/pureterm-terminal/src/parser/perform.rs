use crate::screen::{Cell, CellAttrs, Color, Cursor, CursorStyle, Grid, NamedColor};
use crate::mode::TermMode;
use crate::hyperlink::Hyperlink;
use crate::mark::Mark;
use crate::charset::{Charset, CharsetIndex, map_char, charset_from_code};
use log::{debug, trace};

/// Performs actions based on VTE escape sequences.
pub struct VtePerform {
    /// The terminal grid.
    pub grid: Grid,
    /// The cursor.
    pub cursor: Cursor,
    /// Terminal mode flags.
    pub mode: TermMode,
    /// Current foreground color.
    pub fg: Color,
    /// Current background color.
    pub bg: Color,
    /// Current cell attributes.
    pub attrs: CellAttrs,
    /// Terminal title.
    pub title: String,
    /// Current directory from OSC 7.
    pub current_dir: Option<String>,
    /// Dirty flag - set when screen content changes.
    pub dirty: bool,
    /// Number of columns.
    pub cols: usize,
    /// Number of rows.
    pub rows: usize,
    /// Saved cursor state (for DECSC/DECRC).
    pub saved_cursor: Option<Cursor>,
    /// Saved attributes.
    pub saved_attrs: Option<(Color, Color, CellAttrs)>,
    /// Scroll region top.
    pub scroll_top: usize,
    /// Scroll region bottom.
    pub scroll_bottom: usize,
    /// Tab stops.
    pub tab_stops: Vec<bool>,
    /// Pending OSC string.
    osc_buffer: String,
    /// Current active hyperlink from OSC 8 (applied to newly printed cells).
    pub current_hyperlink: Option<std::sync::Arc<Hyperlink>>,
    /// Alternate screen buffer (saved when switching to alt screen).
    alt_buffer: Option<AltBufferData>,
    /// Pending wrap flag: when cursor is at right margin, next printable char wraps.
    pending_wrap: bool,
    /// Response writer callback: sends data back to the child process.
    response_writer: Option<Box<dyn FnMut(&[u8]) + Send>>,
    /// DCS/APC data accumulator.
    dcs_buffer: Vec<u8>,
    /// DCS/APC intermediates from hook.
    dcs_intermediates: Vec<u8>,
    /// DCS/APC action char from hook.
    dcs_action: Option<char>,
    /// Last printed character (for REP sequence).
    last_char: Option<char>,
    /// Bell triggered flag (set on BEL, cleared by consumer).
    pub bell_triggered: bool,
    /// Clipboard callback for OSC 52 clipboard access.
    clipboard_callback: Option<Box<dyn FnMut(&str) + Send>>,
    /// Dynamic palette colors (OSC 4). None means default.
    dynamic_colors: [Option<Color>; 256],
    /// Cursor color (OSC 12).
    cursor_color: Option<Color>,
    /// Character sets (G0-G3).
    charsets: [Option<Charset>; 4],
    /// Current GL (left side) charset index.
    gl_charset: CharsetIndex,
    /// Current GR (right side) charset index.
    gr_charset: CharsetIndex,
    /// Single shift state (SS2/SS3).
    shift_state: Option<CharsetIndex>,
    /// Left margin for DECSLRM.
    left_margin: usize,
    /// Right margin for DECSLRM.
    right_margin: usize,
}

/// Saved alternate buffer data.
struct AltBufferData {
    grid: Grid,
    cursor: Cursor,
    saved_cursor: Option<Cursor>,
    saved_attrs: Option<(Color, Color, CellAttrs)>,
    scroll_top: usize,
    scroll_bottom: usize,
}

impl VtePerform {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let grid = Grid::new(cols, rows, scrollback);
        let tab_stops = (0..cols).map(|i| i % 8 == 0).collect();
        Self {
            scroll_top: 0,
            scroll_bottom: rows,
            grid,
            cursor: Cursor::new(),
            mode: {
                let mut m = TermMode::default();
                m.insert(TermMode::SHOW_CURSOR);
                m.insert(TermMode::AUTO_WRAP);
                m
            },
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            attrs: CellAttrs::default(),
            title: String::new(),
            current_dir: None,
            dirty: true,
            cols,
            rows,
            saved_cursor: None,
            saved_attrs: None,
            tab_stops,
            osc_buffer: String::new(),
            current_hyperlink: None,
            alt_buffer: None,
            pending_wrap: false,
            response_writer: None,
            dcs_buffer: Vec::new(),
            dcs_intermediates: Vec::new(),
            dcs_action: None,
            last_char: None,
            bell_triggered: false,
            clipboard_callback: None,
            dynamic_colors: [None; 256],
            cursor_color: None,
            charsets: [None, None, None, None],
            gl_charset: CharsetIndex::G0,
            gr_charset: CharsetIndex::G1,
            shift_state: None,
            left_margin: 0,
            right_margin: cols,
        }
    }

    /// Set the response writer callback for sending data back to the child process.
    pub fn set_response_writer(&mut self, writer: Box<dyn FnMut(&[u8]) + Send>) {
        self.response_writer = Some(writer);
    }

    /// Set the clipboard callback for OSC 52 clipboard access.
    pub fn set_clipboard_callback(&mut self, callback: Box<dyn FnMut(&str) + Send>) {
        self.clipboard_callback = Some(callback);
    }

    /// Get the current cursor color (OSC 12).
    pub fn cursor_color(&self) -> Option<Color> {
        self.cursor_color
    }

    /// Get the dynamic color for a palette index (OSC 4).
    pub fn get_dynamic_color(&self, idx: u8) -> Option<Color> {
        self.dynamic_colors[idx as usize]
    }

    /// Send a response back to the child process.
    fn respond(&mut self, data: &[u8]) {
        if let Some(ref mut writer) = self.response_writer {
            writer(data);
        }
    }

    /// Save cursor state for alternate screen switch.
    fn save_cursor_for_alt(&mut self) {
        self.saved_cursor = Some(self.cursor.clone());
        self.saved_attrs = Some((self.fg, self.bg, self.attrs));
    }

    /// Restore cursor state from alternate screen switch.
    fn restore_cursor_from_alt(&mut self) {
        if let Some(saved) = self.saved_cursor.take() {
            self.cursor = saved;
        }
        if let Some((fg, bg, attrs)) = self.saved_attrs.take() {
            self.fg = fg;
            self.bg = bg;
            self.attrs = attrs;
        }
    }

    /// Switch to alternate screen buffer.
    fn switch_to_alt_buffer(&mut self) {
        if self.alt_buffer.is_some() {
            return; // Already on alt buffer
        }
        self.alt_buffer = Some(AltBufferData {
            grid: std::mem::replace(&mut self.grid, Grid::new(self.cols, self.rows, 0)),
            cursor: self.cursor.clone(),
            saved_cursor: self.saved_cursor.take(),
            saved_attrs: self.saved_attrs.take(),
            scroll_top: self.scroll_top,
            scroll_bottom: self.scroll_bottom,
        });
        // Alt buffer has no scrollback
        self.grid = Grid::new(self.cols, self.rows, 0);
        self.cursor.row = 0;
        self.cursor.col = 0;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows;
        self.pending_wrap = false;
    }

    /// Switch back to primary screen buffer.
    fn switch_to_primary_buffer(&mut self) {
        if let Some(alt) = self.alt_buffer.take() {
            self.grid = alt.grid;
            // cursor and saved state are restored via restore_cursor_from_alt()
            // (called by mode handler after this)
            self.scroll_top = alt.scroll_top;
            self.scroll_bottom = alt.scroll_bottom;
        }
    }

    /// Resize the terminal.
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        self.grid.resize(new_cols, new_rows);
        if let Some(ref mut alt) = self.alt_buffer {
            alt.grid.resize(new_cols, new_rows);
        }
        self.cols = new_cols;
        self.rows = new_rows;
        self.scroll_bottom = new_rows;
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(new_cols.saturating_sub(1));
        self.tab_stops = (0..new_cols).map(|i| i % 8 == 0).collect();
        self.dirty = true;
    }

    /// Get a snapshot of the visible screen.
    pub fn snapshot(&self) -> ScreenSnapshot {
        let visible = self.grid.visible_cells();
        let rows: Vec<Vec<Cell>> = visible.iter().map(|row| row.to_vec()).collect();
        ScreenSnapshot {
            rows,
            cursor_row: self.cursor.row,
            cursor_col: self.cursor.col,
            cursor_visible: self.cursor.visible && self.mode.contains(TermMode::SHOW_CURSOR),
            cursor_style: self.cursor.style,
        }
    }

    fn scroll_region_up(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom > self.rows {
            return;
        }
        if top == 0 && bottom == self.rows {
            // Full screen scroll
            self.grid.scroll_up();
        } else {
            // Scroll within the region: delete the top row, insert blank at bottom
            self.grid.scroll_region_up(top, bottom);
        }
    }

    fn scroll_region_down(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom > self.rows {
            return;
        }
        if top == 0 && bottom == self.rows {
            self.grid.scroll_down();
        } else {
            self.grid.scroll_region_down(top, bottom);
        }
    }

    fn put_char_at_cursor(&mut self, mut c: char) {
        let width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width == 0 {
            return;
        }

        // Apply character set mapping
        let charset_opt = match self.shift_state {
            Some(CharsetIndex::G2) => self.charsets[2],
            Some(CharsetIndex::G3) => self.charsets[3],
            _ => {
                // Use GL charset (G0 or G1 based on lock shift)
                if self.gl_charset == CharsetIndex::G0 {
                    self.charsets[0]
                } else {
                    self.charsets[1]
                }
            }
        };
        if let Some(charset) = charset_opt {
            c = map_char(charset, c);
        }

        // Clear single shift state after processing
        self.shift_state = None;

        // Handle pending wrap from previous character
        if self.pending_wrap {
            if self.mode.contains(TermMode::AUTO_WRAP) {
                // Mark the previous row as wrapped before advancing
                self.mark_row_wrapped();
                let bottom = self.scroll_bottom.min(self.rows);
                if self.cursor.row + 1 >= bottom {
                    self.scroll_region_up(self.scroll_top, bottom);
                } else {
                    self.cursor.row += 1;
                }
                self.cursor.col = 0;
            } else {
                // No wrap: overwrite last column
                self.cursor.col = self.cols - 1;
            }
            self.pending_wrap = false;
        }

        // IRM: Insert mode shifts existing characters right
        if self.mode.contains(TermMode::INSERT) {
            if let Some(row) = self.grid.visible_row_mut(self.cursor.row) {
                let col = self.cursor.col;
                for i in (col + 1..self.cols).rev() {
                    row[i] = row[i - 1].clone();
                }
                row[col].reset();
            }
        }

        if let Some(cell) = self.grid.get_mut(self.cursor.row, self.cursor.col) {
            cell.c = c;
            cell.fg = self.fg;
            cell.bg = self.bg;
            cell.attrs = self.attrs;
            cell.hyperlink = self.current_hyperlink.clone();
        }

        if width == 2 && self.cursor.col + 1 < self.cols {
            if let Some(cell) = self.grid.get_mut(self.cursor.row, self.cursor.col + 1) {
                cell.c = ' ';
                cell.fg = self.fg;
                cell.bg = self.bg;
                cell.attrs = CellAttrs {
                    double_width: true,
                    ..self.attrs
                };
            }
        }

        self.cursor.col += width;
        if self.cursor.col >= self.cols {
            self.cursor.col = self.cols;
            self.pending_wrap = true;
        }
        self.dirty = true;
    }

    /// Mark the current row as wrapped (called when pending_wrap triggers a line break).
    fn mark_row_wrapped(&mut self) {
        self.grid.set_wrapped(self.cursor.row, true);
    }
}

/// A snapshot of the screen state for rendering.
#[derive(Debug, Clone)]
pub struct ScreenSnapshot {
    pub rows: Vec<Vec<Cell>>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub cursor_visible: bool,
    pub cursor_style: crate::screen::CursorStyle,
}

impl vte::Perform for VtePerform {
    fn print(&mut self, c: char) {
        self.put_char_at_cursor(c);
        self.last_char = Some(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => {
                // BEL - Bell
                self.bell_triggered = true;
                if self.mode.contains(TermMode::VISIBLE_BELL) {
                    log::debug!("Visual bell triggered");
                } else {
                    log::trace!("BEL received");
                }
            }
            0x08 => {
                // BS - Backspace
                self.pending_wrap = false;
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
                self.dirty = true;
            }
            0x09 => {
                // HT - Horizontal Tab
                let mut next = self.cursor.col + 1;
                while next < self.cols && !self.tab_stops[next] {
                    next += 1;
                }
                self.cursor.col = next.min(self.cols - 1);
                self.dirty = true;
            }
            0x0a => {
                // LF - Line Feed
                self.pending_wrap = false;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                if self.cursor.row + 1 >= bottom {
                    if self.cursor.row >= top {
                        self.scroll_region_up(top, bottom);
                    }
                } else {
                    self.cursor.row += 1;
                }
                if self.mode.contains(TermMode::LINE_FEED_NEW_LINE) {
                    self.cursor.col = 0;
                }
                self.dirty = true;
            }
            0x0d => {
                // CR - Carriage Return
                self.pending_wrap = false;
                self.cursor.col = 0;
                self.dirty = true;
            }
            _ => {
                trace!("Unhandled execute byte: 0x{:02x}", byte);
            }
        }
    }

    fn hook(&mut self, _params: &vte::Params, intermediates: &[u8], _ignore: bool, action: char) {
        // Start accumulating DCS/APC/PM data
        self.dcs_buffer.clear();
        self.dcs_intermediates = intermediates.to_vec();
        self.dcs_action = Some(action);
    }

    fn put(&mut self, byte: u8) {
        self.dcs_buffer.push(byte);
    }

    fn unhook(&mut self) {
        let action = self.dcs_action.take();
        let intermediates = std::mem::take(&mut self.dcs_intermediates);
        let data = std::mem::take(&mut self.dcs_buffer);

        match action {
            Some('q') if intermediates.contains(&0x24) => {
                // DECRQSS (Request Status String) - DCS $ q ... ST
                // Format: DCS $ q request ST
                // Responses: DCS 1 $ request value ST
                if let Ok(request) = String::from_utf8(data.clone()) {
                    let request = request.trim();
                    debug!("DECRQSS request: {}", request);
                    let response = match request {
                        "m" => {
                            // SGR status
                            self.get_sgr_status()
                        }
                        "r" => {
                            // DECSTBM status
                            self.get_decstbm_status()
                        }
                        " q" => {
                            // DECSCUSR status (cursor style)
                            self.get_decscusr_status()
                        }
                        _ => {
                            debug!("Unknown DECRQSS request: {}", request);
                            String::new()
                        }
                    };
                    if !response.is_empty() {
                        // Response format: DCS 1 $ request value ST
                        // DCS P s ... ST where P=1 is the response prefix
                        let dcs_response = format!("\x1bP1${}{}\x1b\\", request, response);
                        self.respond(dcs_response.as_bytes());
                    }
                }
            }
            Some('q') if intermediates.is_empty() || intermediates == [0x20] => {
                // Sixel image data (DCS ... q)
                // Data is accumulated in self.dcs_buffer
                // For now, log that we received it; full Sixel rendering
                // requires connecting to the SixelParser in image.rs
                log::debug!(
                    "Sixel DCS received: {} bytes (rendering not yet connected)",
                    data.len()
                );
            }
            Some('G') if intermediates == [0x1F] || intermediates.is_empty() => {
                // APC with action 'G' is Kitty graphics (ESC _ G ... ST)
                // Note: in vte crate, APC may arrive through hook/put/unhook
                log::debug!(
                    "Kitty APC received: {} bytes (rendering not yet connected)",
                    data.len()
                );
            }
            Some('u') => {
                // DEC User Defined Keys
                log::debug!("DECUDK received: {} bytes", data.len());
            }
            _ => {
                if let Some(a) = action {
                    debug!(
                        "DCS/APC/PM unhook: action='{}' intermediates={:?} data={} bytes",
                        a as char, intermediates, data.len()
                    );
                }
            }
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.is_empty() {
            return;
        }

        let terminator = params.last().unwrap();
        let param0 = String::from_utf8_lossy(params[0]);

        match param0.as_ref() {
            "0" | "2" => {
                // Set window title
                if params.len() > 1 {
                    self.title = String::from_utf8_lossy(
                        &params[1..].iter().flat_map(|p| p.iter().copied()).collect::<Vec<_>>()
                    ).into();
                }
            }
            "7" => {
                // Set current directory (OSC 7)
                if params.len() > 1 {
                    self.current_dir = Some(String::from_utf8_lossy(
                        &params[1..].iter().flat_map(|p| p.iter().copied()).collect::<Vec<_>>()
                    ).into());
                }
            }
            "8" => {
                // OSC 8: Hyperlinks
                // Format: OSC 8 ; params ; uri ST
                // params can be empty or "id=..."
                // To close: OSC 8 ; ; ST
                if params.len() > 1 {
                    let combined: Vec<u8> = params[1..]
                        .iter()
                        .flat_map(|p| p.iter().copied())
                        .collect();

                    if let Some(semi_pos) = combined.iter().position(|&b| b == b';') {
                        let params_str = String::from_utf8_lossy(&combined[..semi_pos]);
                        let uri = String::from_utf8_lossy(&combined[semi_pos + 1..]).to_string();

                        if uri.is_empty() {
                            // Close hyperlink
                            self.current_hyperlink = None;
                        } else {
                            let params = if params_str.is_empty() {
                                None
                            } else {
                                Some(params_str.to_string())
                            };
                            self.current_hyperlink = Some(std::sync::Arc::new(
                                Hyperlink::new(uri, params)
                            ));
                        }
                    } else {
                        // No semicolon found - just a close command
                        self.current_hyperlink = None;
                    }
                } else {
                    self.current_hyperlink = None;
                }
            }
            "52" => {
                // OSC 52 - Clipboard access
                // Format: OSC 52 ; Pc ; Pd ST
                // Pc: 0=clear, 1=write clipboard, 2=read clipboard, 3=write selection, 4=read selection
                // Pd: Base64 encoded content
                if params.len() >= 2 {
                    let op = String::from_utf8_lossy(params[1]).parse::<u8>().unwrap_or(0);
                    let data: Vec<u8> = params.get(2).map(|p| p.to_vec()).unwrap_or_default();

                    match op {
                        0 | 1 => {
                            // Write to clipboard (0=clipboard, 1=clipboard)
                            if let Some(ref mut cb) = self.clipboard_callback {
                                if let Ok(text) = base64_decode(&data) {
                                    cb(text.as_str());
                                }
                            }
                        }
                        2 => {
                            // Read from clipboard
                            if let Some(ref mut cb) = self.clipboard_callback {
                                cb(""); // Request clipboard content
                            }
                        }
                        3 | 4 => {
                            // Write/read selection - treat same as clipboard for now
                            if let Some(ref mut cb) = self.clipboard_callback {
                                if let Ok(text) = base64_decode(&data) {
                                    cb(text.as_str());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "4" => {
                // OSC 4 - Change dynamic colors
                // Format: OSC 4 ; c ; Pt ST
                if params.len() >= 3 {
                    let idx = String::from_utf8_lossy(params[1]).parse::<usize>().unwrap_or(0);
                    let color = parse_osc_color(params.get(2..).unwrap_or(&[]));
                    if idx < 256 {
                        self.dynamic_colors[idx] = Some(color);
                        self.dirty = true;
                    }
                }
            }
            "10" => {
                // OSC 10 - Set foreground color
                if params.len() >= 2 {
                    self.fg = parse_osc_color(params.get(1..).unwrap_or(&[]));
                    self.dirty = true;
                }
            }
            "11" => {
                // OSC 11 - Set background color
                if params.len() >= 2 {
                    self.bg = parse_osc_color(params.get(1..).unwrap_or(&[]));
                    self.dirty = true;
                }
            }
            "12" => {
                // OSC 12 - Set cursor color
                if params.len() >= 2 {
                    self.cursor_color = Some(parse_osc_color(params.get(1..).unwrap_or(&[])));
                    self.dirty = true;
                }
            }
            _ => {
                // OSC 133: Shell integration marks
                if param0.starts_with("133") {
                    let marker: String = String::from_utf8_lossy(
                        &params[1..].iter().flat_map(|p| p.iter().copied()).collect::<Vec<_>>()
                    ).into();
                    let marker_trimmed = marker.trim();
                    if !marker_trimmed.is_empty() {
                        if let Some(mark) = Mark::from_osc133(marker_trimmed) {
                            self.grid.set_mark(self.cursor.row, mark);
                        }
                    } else if param0 == "133" {
                        // OSC 133 ; A/B/C/D with empty marker part — try the param0 suffix
                        // param0 could be "133;A" depending on how vte splits
                    }
                }
                debug!("Unhandled OSC: {}", param0);
            }
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, action: char) {
        let params_vec: Vec<Vec<u16>> = params.iter().map(|p| p.to_vec()).collect();
        let is_dec_private = intermediates == [0x3F]; // '?' prefix

        match action {
            'A' => {
                // CUU - Cursor Up
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.row = self.cursor.row.saturating_sub(n);
                self.pending_wrap = false;
            }
            'B' => {
                // CUD - Cursor Down
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.row = (self.cursor.row + n).min(self.rows - 1);
                self.pending_wrap = false;
            }
            'C' => {
                // CUF - Cursor Forward
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.col = (self.cursor.col + n).min(self.cols - 1);
                self.pending_wrap = false;
            }
            'D' => {
                // CUB - Cursor Back
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.col = self.cursor.col.saturating_sub(n);
                self.pending_wrap = false;
            }
            'H' | 'f' => {
                // CUP - Cursor Position
                let row = param_or(&params_vec, 0, 1).saturating_sub(1) as usize;
                let col = param_or(&params_vec, 1, 1).saturating_sub(1) as usize;
                if self.mode.contains(TermMode::ORIGIN) {
                    self.cursor.row = (row + self.scroll_top).min(self.scroll_bottom.saturating_sub(1));
                } else {
                    self.cursor.row = row.min(self.rows - 1);
                }
                self.cursor.col = col.min(self.cols - 1);
                self.pending_wrap = false;
            }
            'J' => {
                // ED - Erase Display
                let mode = param_or(&params_vec, 0, 0);
                match mode {
                    0 => {
                        // Erase from cursor to end
                        for c in self.cursor.col..self.cols {
                            if let Some(cell) = self.grid.get_mut(self.cursor.row, c) {
                                cell.reset();
                            }
                        }
                        for r in (self.cursor.row + 1)..self.rows {
                            for c in 0..self.cols {
                                if let Some(cell) = self.grid.get_mut(r, c) {
                                    cell.reset();
                                }
                            }
                        }
                    }
                    1 => {
                        // Erase from start to cursor
                        for r in 0..self.cursor.row {
                            for c in 0..self.cols {
                                if let Some(cell) = self.grid.get_mut(r, c) {
                                    cell.reset();
                                }
                            }
                        }
                        for c in 0..=self.cursor.col {
                            if let Some(cell) = self.grid.get_mut(self.cursor.row, c) {
                                cell.reset();
                            }
                        }
                    }
                    2 => {
                        // Erase entire display
                        self.grid.clear();
                    }
                    _ => {}
                }
                self.dirty = true;
            }
            'K' => {
                // EL - Erase Line
                let mode = param_or(&params_vec, 0, 0);
                match mode {
                    0 => {
                        // Erase from cursor to end of line
                        for c in self.cursor.col..self.cols {
                            if let Some(cell) = self.grid.get_mut(self.cursor.row, c) {
                                cell.reset();
                            }
                        }
                    }
                    1 => {
                        // Erase from start to cursor
                        for c in 0..=self.cursor.col {
                            if let Some(cell) = self.grid.get_mut(self.cursor.row, c) {
                                cell.reset();
                            }
                        }
                    }
                    2 => {
                        // Erase entire line
                        if let Some(row) = self.grid.visible_row_mut(self.cursor.row) {
                            for cell in row.iter_mut() {
                                cell.reset();
                            }
                        }
                    }
                    _ => {}
                }
                self.dirty = true;
            }
            'L' => {
                // IL - Insert Lines
                let n = param_or(&params_vec, 0, 1) as usize;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                for _ in 0..n {
                    self.scroll_region_down(top, bottom);
                }
                self.dirty = true;
            }
            'M' => {
                // DL - Delete Lines
                let n = param_or(&params_vec, 0, 1) as usize;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                for _ in 0..n {
                    self.scroll_region_up(top, bottom);
                }
                self.dirty = true;
            }
            'P' => {
                // DCH - Delete Characters
                let n = param_or(&params_vec, 0, 1) as usize;
                if let Some(row) = self.grid.visible_row_mut(self.cursor.row) {
                    for i in self.cursor.col..self.cols {
                        let src = i + n;
                        if src < self.cols {
                            row[i] = row[src].clone();
                        } else {
                            row[i].reset();
                        }
                    }
                }
                self.dirty = true;
            }
            'S' => {
                // SU - Scroll Up
                let n = param_or(&params_vec, 0, 1) as usize;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                for _ in 0..n {
                    self.scroll_region_up(top, bottom);
                }
                self.dirty = true;
            }
            'T' => {
                // SD - Scroll Down
                let n = param_or(&params_vec, 0, 1) as usize;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                for _ in 0..n {
                    self.scroll_region_down(top, bottom);
                }
                self.dirty = true;
            }
            'X' => {
                // ECH - Erase Characters
                let n = param_or(&params_vec, 0, 1) as usize;
                for i in 0..n {
                    let col = self.cursor.col + i;
                    if col >= self.cols {
                        break;
                    }
                    if let Some(cell) = self.grid.get_mut(self.cursor.row, col) {
                        cell.reset();
                    }
                }
                self.dirty = true;
            }
            'm' => {
                // SGR - Select Graphic Rendition
                self.handle_sgr(&params_vec);
            }
            'h' => {
                // SM - Set Mode
                if is_dec_private {
                    self.handle_set_dec_mode(&params_vec);
                } else {
                    self.handle_set_ansi_mode(&params_vec);
                }
            }
            'l' => {
                // RM - Reset Mode
                if is_dec_private {
                    self.handle_reset_dec_mode(&params_vec);
                } else {
                    self.handle_reset_ansi_mode(&params_vec);
                }
            }
            's' => {
                // DECSLRM (Set Left and Right Margin) when DECLRMM is set
                // Format: CSI Pl;Pr s
                // Otherwise: Save cursor position (DECSC)
                if self.mode.contains(TermMode::LEFT_RIGHT_MARGIN)
                    && params_vec.len() >= 2
                    && !(params_vec[0].is_empty() && params_vec[1].is_empty())
                {
                    // Set left margin
                    let left = if params_vec[0].is_empty() {
                        0
                    } else {
                        param_or(&params_vec, 0, 1).saturating_sub(1) as usize
                    };
                    // Set right margin
                    let right = if params_vec[1].is_empty() {
                        self.cols
                    } else {
                        param_or(&params_vec, 1, self.cols as u16) as usize
                    };
                    if left < right && right <= self.cols {
                        self.left_margin = left;
                        self.right_margin = right;
                    }
                } else {
                    // Save cursor position
                    self.saved_cursor = Some(self.cursor.clone());
                }
            }
            'u' => {
                // Restore cursor position
                if let Some(saved) = &self.saved_cursor {
                    self.cursor = saved.clone();
                }
            }
            'r' => {
                // DECSTBM - Set Scrolling Region
                let top = param_or(&params_vec, 0, 1).saturating_sub(1) as usize;
                let bottom = param_or(&params_vec, 1, self.rows as u16) as usize;
                if top < bottom && bottom <= self.rows {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                    self.cursor.row = if self.mode.contains(TermMode::ORIGIN) {
                        top
                    } else {
                        0
                    };
                    self.cursor.col = 0;
                }
            }
            'G' => {
                // CHA - Cursor Horizontal Absolute
                let col = param_or(&params_vec, 0, 1).saturating_sub(1) as usize;
                self.cursor.col = col.min(self.cols - 1);
                self.pending_wrap = false;
            }
            'd' => {
                // VPA - Vertical Position Absolute
                let row = param_or(&params_vec, 0, 1).saturating_sub(1) as usize;
                self.cursor.row = row.min(self.rows - 1);
                self.pending_wrap = false;
            }
            'q' => {
                // DECSCUSR - Set Cursor Style
                // CSI Ps SP q  (intermediates contains SP = 0x20)
                // Accept both with and without space intermediate for compatibility
                let style = param_or(&params_vec, 0, 0);
                match style {
                    0 | 1 => { self.cursor.style = CursorStyle::Block; self.cursor.blink = true; }
                    2 => { self.cursor.style = CursorStyle::Block; self.cursor.blink = false; }
                    3 => { self.cursor.style = CursorStyle::Underline; self.cursor.blink = true; }
                    4 => { self.cursor.style = CursorStyle::Underline; self.cursor.blink = false; }
                    5 => { self.cursor.style = CursorStyle::Beam; self.cursor.blink = true; }
                    6 => { self.cursor.style = CursorStyle::Beam; self.cursor.blink = false; }
                    _ => {}
                }
            }
            '@' => {
                // ICH - Insert Characters
                let n = param_or(&params_vec, 0, 1) as usize;
                if let Some(row) = self.grid.visible_row_mut(self.cursor.row) {
                    // Shift characters right, fill with blanks
                    let col = self.cursor.col;
                    for i in (col + n..self.cols).rev() {
                        if i >= n && i - n < self.cols {
                            row[i] = row[i - n].clone();
                        }
                    }
                    for i in col..(col + n).min(self.cols) {
                        row[i].reset();
                    }
                }
                self.dirty = true;
            }
            'I' => {
                // CHT - Cursor Horizontal Forward Tabulation
                let n = param_or(&params_vec, 0, 1) as usize;
                for _ in 0..n {
                    let mut next = self.cursor.col + 1;
                    while next < self.cols && !self.tab_stops.get(next).copied().unwrap_or(false) {
                        next += 1;
                    }
                    self.cursor.col = next.min(self.cols - 1);
                }
                self.pending_wrap = false;
                self.dirty = true;
            }
            'Z' => {
                // CBT - Cursor Backward Tabulation
                let n = param_or(&params_vec, 0, 1) as usize;
                for _ in 0..n {
                    if self.cursor.col > 0 {
                        let mut prev = self.cursor.col - 1;
                        while prev > 0 && !self.tab_stops.get(prev).copied().unwrap_or(false) {
                            prev -= 1;
                        }
                        self.cursor.col = prev;
                    }
                }
                self.pending_wrap = false;
                self.dirty = true;
            }
            'E' => {
                // CNL - Cursor Next Line
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.row = (self.cursor.row + n).min(self.rows - 1);
                self.cursor.col = 0;
                self.pending_wrap = false;
            }
            'F' => {
                // CPL - Cursor Previous Line
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.row = self.cursor.row.saturating_sub(n);
                self.cursor.col = 0;
                self.pending_wrap = false;
            }
            'g' => {
                // TBC - Tab Clear
                let mode = param_or(&params_vec, 0, 0);
                match mode {
                    0 => {
                        // Clear tab stop at cursor
                        if self.cursor.col < self.tab_stops.len() {
                            self.tab_stops[self.cursor.col] = false;
                        }
                    }
                    3 => {
                        // Clear all tab stops
                        for stop in &mut self.tab_stops {
                            *stop = false;
                        }
                    }
                    _ => {}
                }
                self.dirty = true;
            }
            'c' => {
                // DA - Device Attributes
                if is_dec_private {
                    // DA1 with ? prefix: CSI ? c -> respond with primary DA
                    self.respond(b"\x1b[?65;22c");
                } else if intermediates == [0x3E] {
                    // DA2 - Secondary Device Attributes: CSI > c
                    self.respond(b"\x1b[>0;1;0c");
                } else {
                    // DA1 without prefix: CSI c
                    self.respond(b"\x1b[1;2c");
                }
            }
            'n' => {
                // DSR - Device Status Report
                if !is_dec_private {
                    let mode = param_or(&params_vec, 0, 0);
                    match mode {
                        5 => {
                            // Device status: OK
                            self.respond(b"\x1b[0n");
                        }
                        6 => {
                            // Cursor Position Report
                            let row = (self.cursor.row + 1) as u16;
                            let col = (self.cursor.col + 1) as u16;
                            let resp = format!("\x1b[{};{}R", row, col);
                            self.respond(resp.as_bytes());
                        }
                        _ => {}
                    }
                }
            }
            'p' => {
                if intermediates == [0x21] {
                    // DECSTR - Soft Terminal Reset (CSI ! p)
                    self.cursor = Cursor::new();
                    self.mode = {
                        let mut m = TermMode::default();
                        m.insert(TermMode::SHOW_CURSOR);
                        m.insert(TermMode::AUTO_WRAP);
                        m
                    };
                    self.fg = Color::Named(NamedColor::Foreground);
                    self.bg = Color::Named(NamedColor::Background);
                    self.attrs = CellAttrs::default();
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows;
                    self.saved_cursor = None;
                    self.saved_attrs = None;
                    self.pending_wrap = false;
                    self.tab_stops = (0..self.cols).map(|i| i % 8 == 0).collect();
                }
            }
            't' => {
                // XTWINOPS - Window Operations
                let mode = param_or(&params_vec, 0, 0);
                match mode {
                    18 => {
                        // Report text area size in characters
                        let resp = format!("\x1b[8;{};{}t", self.rows, self.cols);
                        self.respond(resp.as_bytes());
                    }
                    22 | 23 => {
                        // Push/Pop title stack - silently accept
                    }
                    _ => {
                        debug!("Unhandled XTWINOPS: CSI {}t", mode);
                    }
                }
            }
            'b' => {
                // REP - Repeat Previous Character
                let n = param_or(&params_vec, 0, 1) as usize;
                if let Some(c) = self.last_char {
                    for _ in 0..n.saturating_sub(1) {
                        self.put_char_at_cursor(c);
                    }
                }
            }
            _ => {
                debug!("Unhandled CSI {} with {:?} params", action, params_vec);
            }
        }
        self.dirty = true;
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'7' => {
                // DECSC - Save cursor position and attributes
                self.cursor.save();
                self.saved_cursor = Some(self.cursor.clone());
                self.saved_attrs = Some((self.fg, self.bg, self.attrs));
            }
            b'8' => {
                // DECRC - Restore cursor position and attributes
                if let Some(saved) = &self.saved_cursor {
                    self.cursor.row = saved.row;
                    self.cursor.col = saved.col;
                    self.cursor.style = saved.style;
                    self.cursor.blink = saved.blink;
                    self.cursor.visible = saved.visible;
                } else {
                    self.cursor.restore();
                }
                self.pending_wrap = false;
                if let Some((fg, bg, attrs)) = &self.saved_attrs {
                    self.fg = *fg;
                    self.bg = *bg;
                    self.attrs = *attrs;
                }
            }
            b'c' => {
                // RIS - Full terminal reset
                self.cursor = Cursor::new();
                self.grid.clear();
                self.mode = TermMode::default();
                self.fg = Color::Named(NamedColor::Foreground);
                self.bg = Color::Named(NamedColor::Background);
                self.attrs = CellAttrs::default();
                self.scroll_top = 0;
                self.scroll_bottom = self.rows;
                self.saved_cursor = None;
                self.saved_attrs = None;
                self.tab_stops = (0..self.cols).map(|i| i % 8 == 0).collect();
            }
            b'D' => {
                // IND - Index (move cursor down, scroll if at bottom)
                self.pending_wrap = false;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                if self.cursor.row + 1 >= bottom {
                    if self.cursor.row >= top {
                        self.scroll_region_up(top, bottom);
                    }
                } else {
                    self.cursor.row += 1;
                }
            }
            b'M' => {
                // RI - Reverse Index (move cursor up, scroll if at top)
                self.pending_wrap = false;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                if self.cursor.row == top {
                    self.scroll_region_down(top, bottom);
                } else if self.cursor.row > 0 {
                    self.cursor.row -= 1;
                }
            }
            b'E' => {
                // NEL - Next Line (CR + LF)
                self.pending_wrap = false;
                self.cursor.col = 0;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                if self.cursor.row + 1 >= bottom {
                    if self.cursor.row >= top {
                        self.scroll_region_up(top, bottom);
                    }
                } else {
                    self.cursor.row += 1;
                }
            }
            b'#'
                if intermediates.is_empty()
                    || intermediates.first() == Some(&b'#') =>
            {
                // DEC Screen Alignment Test (ESC # 8)
                // The '#' intermediate is passed via the intermediates array by vte,
                // but some parser variants encode it differently. Handle ESC # 8
                // by filling the screen with 'E' characters.
                for row in 0..self.rows {
                    for col in 0..self.cols {
                        if let Some(cell) = self.grid.get_mut(row, col) {
                            cell.c = 'E';
                        }
                    }
                }
            }
            b'(' => {
                // Designate G0 character set - ESC ( C
                if let Some(&_intermediate) = intermediates.first() {
                    self.charsets[0] = Some(charset_from_code(byte, intermediates));
                } else {
                    self.charsets[0] = Some(charset_from_code(byte, intermediates));
                }
            }
            b')' => {
                // Designate G1 character set - ESC ) C
                self.charsets[1] = Some(charset_from_code(byte, intermediates));
            }
            b'*' => {
                // Designate G2 character set - ESC * C
                self.charsets[2] = Some(charset_from_code(byte, intermediates));
            }
            b'+' => {
                // Designate G3 character set - ESC + C
                self.charsets[3] = Some(charset_from_code(byte, intermediates));
            }
            b'N' => {
                // SS2 - Single Shift 2 (shift G2 for next character only)
                self.shift_state = Some(CharsetIndex::G2);
            }
            b'O' => {
                // SS3 - Single Shift 3 (shift G3 for next character only)
                self.shift_state = Some(CharsetIndex::G3);
            }
            b'n' => {
                // LS2 - Locking Shift 2 (shift G2 into GL)
                self.gl_charset = CharsetIndex::G2;
            }
            b'o' => {
                // LS3 - Locking Shift 3 (shift G3 into GL)
                self.gl_charset = CharsetIndex::G3;
            }
            b'0' => {
                // LS1R - Locking Shift 1 Right (shift G1 into GR)
                self.gr_charset = CharsetIndex::G1;
            }
            b'}' => {
                // LS2R - Locking Shift 2 Right (shift G2 into GR)
                self.gr_charset = CharsetIndex::G2;
            }
            b'|' => {
                // LS3R - Locking Shift 3 Right (shift G3 into GR)
                self.gr_charset = CharsetIndex::G3;
            }
            _ => {
                debug!(
                    "Unhandled ESC sequence: intermediates={:?}, byte=0x{:02x} ('{}')",
                    intermediates,
                    byte,
                    (byte as char)
                );
            }
        }
        self.dirty = true;
    }
}

fn param_or(params: &[Vec<u16>], index: usize, default: u16) -> u16 {
    params.get(index).and_then(|p| p.first()).copied().unwrap_or(default)
}

/// Decode base64 string to UTF-8 text.
fn base64_decode(data: &[u8]) -> Result<String, ()> {
    // Base64 decoding table
    fn decode_char(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            b'=' => None, // Padding
            _ => None,
        }
    }

    let mut result = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;

    for &b in data {
        if b == b'\n' || b == b'\r' || b == b' ' || b == b'\t' {
            continue; // Skip whitespace
        }
        if let Some(value) = decode_char(b) {
            buffer = (buffer << 6) | (value as u32);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                result.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        } else if b != b'=' {
            return Err(()); // Invalid character
        }
    }

    String::from_utf8(result).map_err(|_| ())
}

/// Parse OSC color specification into a Color.
/// Supported formats:
/// - "rgb:RR/GG/BB" (hex bytes)
/// - "rgb:RRR/GGG/BBB" (3-digit hex)
/// - "palette#n" or "#RGB" (legacy)
/// - "?" query (returns NamedColor::Foreground)
fn parse_osc_color(params: &[&[u8]]) -> Color {
    if params.is_empty() {
        return Color::Named(NamedColor::Foreground);
    }

    let spec = String::from_utf8_lossy(params[0]);

    // Query
    if spec == "?" {
        return Color::Named(NamedColor::Foreground);
    }

    // Palette index
    if spec.starts_with("palette#") {
        if let Ok(idx) = spec[8..].parse::<u8>() {
            return Color::Palette(idx);
        }
    }

    // Legacy #RGB format
    if spec.starts_with('#') && spec.len() == 4 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&spec[1..2], 16),
            u8::from_str_radix(&spec[2..3], 16),
            u8::from_str_radix(&spec[3..4], 16),
        ) {
            return Color::Rgb(r * 17, g * 17, b * 17);
        }
    }

    // rgb:RR/GG/BB format
    if spec.starts_with("rgb:") {
        let rest = &spec[4..];

        // Try RR/GG/BB format (2 hex digits each)
        if rest.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&rest[0..2], 16),
                u8::from_str_radix(&rest[2..4], 16),
                u8::from_str_radix(&rest[4..6], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        }

        // Try RRR/GGG/BBB format (3 hex digits each)
        if rest.len() == 9 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&rest[0..3], 16),
                u8::from_str_radix(&rest[3..6], 16),
                u8::from_str_radix(&rest[6..9], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        }
    }

    // Default to foreground
    Color::Named(NamedColor::Foreground)
}

/// Parse extended color for SGR (38/48).
fn parse_extended_color(param: &[u16]) -> Option<Color> {
    if param.len() < 2 {
        return None;
    }
    match param[1] {
        5 => {
            // 256-color mode
            if param.len() >= 3 {
                Some(Color::Palette(param[2] as u8))
            } else {
                None
            }
        }
        2 => {
            // Truecolor (RGB)
            if param.len() >= 5 {
                Some(Color::Rgb(param[2] as u8, param[3] as u8, param[4] as u8))
            } else {
                None
            }
        }
        _ => None,
    }
}

impl VtePerform {
    fn handle_sgr(&mut self, params: &[Vec<u16>]) {
        if params.is_empty() {
            self.fg = Color::Named(NamedColor::Foreground);
            self.bg = Color::Named(NamedColor::Background);
            self.attrs = CellAttrs::default();
            return;
        }

        for param in params {
            let code = param.first().copied().unwrap_or(0);
            match code {
                0 => {
                    self.fg = Color::Named(NamedColor::Foreground);
                    self.bg = Color::Named(NamedColor::Background);
                    self.attrs = CellAttrs::default();
                }
                1 => self.attrs.bold = true,
                2 => self.attrs.dim = true,
                3 => self.attrs.italic = true,
                4 => self.attrs.underline = true,
                7 => self.attrs.inverse = true,
                8 => self.attrs.hidden = true,
                9 => self.attrs.strikethrough = true,
                22 => { self.attrs.bold = false; self.attrs.dim = false; }
                23 => self.attrs.italic = false,
                24 => self.attrs.underline = false,
                27 => self.attrs.inverse = false,
                28 => self.attrs.hidden = false,
                29 => self.attrs.strikethrough = false,
                30..=37 => self.fg = Color::Palette((code - 30) as u8),
                38 => {
                    // Extended foreground color
                    if let Some(color) = parse_extended_color(param) {
                        self.fg = color;
                    }
                }
                39 => self.fg = Color::Named(NamedColor::Foreground),
                40..=47 => self.bg = Color::Palette((code - 40) as u8),
                48 => {
                    if let Some(color) = parse_extended_color(param) {
                        self.bg = color;
                    }
                }
                49 => self.bg = Color::Named(NamedColor::Background),
                90..=97 => self.fg = Color::Palette((code - 90 + 8) as u8),
                100..=107 => self.bg = Color::Palette((code - 100 + 8) as u8),
                _ => {}
            }
        }
    }

    /// ANSI Set Mode (CSI Pn h, no '?' prefix)
    fn handle_set_ansi_mode(&mut self, params: &[Vec<u16>]) {
        for param in params {
            if let Some(&mode) = param.first() {
                match mode {
                    4 => {
                        // IRM - Insert Mode
                        self.mode.insert(TermMode::INSERT);
                    }
                    20 => {
                        // LNM - Line Feed New Line Mode
                        self.mode.insert(TermMode::LINE_FEED_NEW_LINE);
                    }
                    _ => debug!("Unhandled ANSI set mode: {}", mode),
                }
            }
        }
    }

    /// ANSI Reset Mode (CSI Pn l, no '?' prefix)
    fn handle_reset_ansi_mode(&mut self, params: &[Vec<u16>]) {
        for param in params {
            if let Some(&mode) = param.first() {
                match mode {
                    4 => {
                        // IRM - Insert Mode off
                        self.mode.remove(TermMode::INSERT);
                    }
                    20 => {
                        // LNM off
                        self.mode.remove(TermMode::LINE_FEED_NEW_LINE);
                    }
                    _ => debug!("Unhandled ANSI reset mode: {}", mode),
                }
            }
        }
    }

    /// DEC Private Set Mode (CSI ? Pn h)
    fn handle_set_dec_mode(&mut self, params: &[Vec<u16>]) {
        for param in params {
            if let Some(&mode) = param.first() {
                match mode {
                    1 => self.mode.insert(TermMode::APP_CURSOR),
                    3 => self.mode.insert(TermMode::COLUMN_132),
                    5 => self.mode.insert(TermMode::INVERSE),
                    6 => self.mode.insert(TermMode::ORIGIN),
                    7 => self.mode.insert(TermMode::AUTO_WRAP),
                    12 => self.mode.insert(TermMode::CURSOR_BLINK),
                    25 => {
                        self.cursor.visible = true;
                        self.mode.insert(TermMode::SHOW_CURSOR);
                    }
                    1000 => self.mode.insert(TermMode::MOUSE_REPORT_CLICK),
                    1002 => self.mode.insert(TermMode::MOUSE_DRAG),
                    1003 => self.mode.insert(TermMode::MOUSE_MOTION),
                    1004 => self.mode.insert(TermMode::FOCUS_IN_OUT),
                    1006 => self.mode.insert(TermMode::MOUSE_SGR),
                    1005 | 1015 => {} // Unsupported mouse modes
                    1047 => {
                        // Switch to alternate screen buffer
                        self.switch_to_alt_buffer();
                    }
                    1048 => {
                        // Save cursor (for alt screen switch)
                        self.save_cursor_for_alt();
                    }
                    1049 => {
                        // Save cursor + switch to alternate screen + clear
                        self.save_cursor_for_alt();
                        self.switch_to_alt_buffer();
                    }
                    2004 => self.mode.insert(TermMode::BRACKETED_PASTE),
                    69 => self.mode.insert(TermMode::LEFT_RIGHT_MARGIN),
                    _ => debug!("Unhandled DEC set mode: ?{}", mode),
                }
            }
        }
    }

    /// DEC Private Reset Mode (CSI ? Pn l)
    fn handle_reset_dec_mode(&mut self, params: &[Vec<u16>]) {
        for param in params {
            if let Some(&mode) = param.first() {
                match mode {
                    1 => self.mode.remove(TermMode::APP_CURSOR),
                    3 => self.mode.remove(TermMode::COLUMN_132),
                    5 => self.mode.remove(TermMode::INVERSE),
                    6 => self.mode.remove(TermMode::ORIGIN),
                    7 => self.mode.remove(TermMode::AUTO_WRAP),
                    12 => self.mode.remove(TermMode::CURSOR_BLINK),
                    25 => {
                        self.cursor.visible = false;
                        self.mode.remove(TermMode::SHOW_CURSOR);
                    }
                    1000 => self.mode.remove(TermMode::MOUSE_REPORT_CLICK),
                    1002 => self.mode.remove(TermMode::MOUSE_DRAG),
                    1003 => self.mode.remove(TermMode::MOUSE_MOTION),
                    1004 => self.mode.remove(TermMode::FOCUS_IN_OUT),
                    1006 => self.mode.remove(TermMode::MOUSE_SGR),
                    1047 => {
                        // Switch back to primary screen buffer
                        self.switch_to_primary_buffer();
                    }
                    1048 => {
                        // Restore cursor
                        self.restore_cursor_from_alt();
                    }
                    1049 => {
                        // Switch back to primary + restore cursor
                        self.switch_to_primary_buffer();
                        self.restore_cursor_from_alt();
                    }
                    2004 => self.mode.remove(TermMode::BRACKETED_PASTE),
                    69 => {
                        self.mode.remove(TermMode::LEFT_RIGHT_MARGIN);
                        self.left_margin = 0;
                        self.right_margin = self.cols;
                    }
                    _ => debug!("Unhandled DEC reset mode: ?{}", mode),
                }
            }
        }
    }

    /// Get current SGR parameters for DECRQSS.
    fn get_sgr_status(&self) -> String {
        let mut sgr = String::new();

        // Bold
        if self.attrs.bold {
            sgr.push_str(";1");
        }
        // Dim
        if self.attrs.dim {
            sgr.push_str(";2");
        }
        // Italic
        if self.attrs.italic {
            sgr.push_str(";3");
        }
        // Underline
        if self.attrs.underline {
            sgr.push_str(";4");
        }
        // Inverse
        if self.attrs.inverse {
            sgr.push_str(";7");
        }
        // Hidden
        if self.attrs.hidden {
            sgr.push_str(";8");
        }
        // Strikethrough
        if self.attrs.strikethrough {
            sgr.push_str(";9");
        }

        // Foreground color
        match self.fg {
            Color::Named(NamedColor::Foreground) => {}
            Color::Named(_) => {
                // Other named colors (Cursor, Selection, etc.) - use default
            }
            Color::Palette(idx) => {
                sgr.push_str(&format!(";38;5;{}", idx));
            }
            Color::Rgb(r, g, b) => {
                sgr.push_str(&format!(";38;2;{};{};{}", r, g, b));
            }
        }

        // Background color
        match self.bg {
            Color::Named(NamedColor::Background) => {}
            Color::Named(_) => {
                // Other named colors - use default
            }
            Color::Palette(idx) => {
                sgr.push_str(&format!(";48;5;{}", idx));
            }
            Color::Rgb(r, g, b) => {
                sgr.push_str(&format!(";48;2;{};{};{}", r, g, b));
            }
        }

        if sgr.is_empty() {
            String::new()
        } else {
            // Remove leading semicolon
            sgr[1..].to_string()
        }
    }

    /// Get current DECSTBM (scrolling region) for DECRQSS.
    fn get_decstbm_status(&self) -> String {
        format!("{};{}r", self.scroll_top + 1, self.scroll_bottom)
    }

    /// Get current DECSCUSR (cursor style) for DECRQSS.
    fn get_decscusr_status(&self) -> String {
        let style = match self.cursor.style {
            CursorStyle::Block => match self.cursor.blink {
                true => "1",
                false => "2",
            },
            CursorStyle::Underline => match self.cursor.blink {
                true => "3",
                false => "4",
            },
            CursorStyle::Beam => match self.cursor.blink {
                true => "5",
                false => "6",
            },
        };
        format!(" {}", style)
    }
}
