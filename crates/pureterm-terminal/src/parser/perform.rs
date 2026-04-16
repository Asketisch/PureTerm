use crate::screen::{Cell, CellAttrs, Color, Cursor, CursorStyle, Grid, NamedColor, UnderlineStyle, WideMode};
use crate::mode::TermMode;
use crate::hyperlink::Hyperlink;
use crate::mark::Mark;
use crate::charset::{Charset, CharsetIndex, map_char, charset_from_designation};
use crate::image::{ImageManager, KittyGraphicsParser};
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
    #[allow(dead_code)]
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
    /// Whether newly written cells are marked as protected.
    cell_protected: bool,
    /// Current protection mode (ISO or DEC).
    protected_mode: ProtectedMode,
    /// Image manager for Kitty graphics and Sixel.
    pub image_manager: ImageManager,
    /// Title stack for CSI 22/23 t push/pop.
    title_stack: Vec<String>,
}

/// Protected mode tracking for cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectedMode {
    /// No protection active.
    Off,
    /// ISO protection (ESC V / ESC W) - all erase ops respect protection.
    Iso,
    /// DEC protection (DECSCA) - only DECSED/DECSEL respect protection.
    Dec,
}

/// Saved alternate buffer data.
#[allow(dead_code)]
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
            cell_protected: false,
            protected_mode: ProtectedMode::Off,
            image_manager: ImageManager::new(),
            title_stack: Vec::new(),
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

    /// Check if a DEC private mode is currently set.
    fn is_dec_mode_set(&self, mode: u16) -> bool {
        match mode {
            1 => self.mode.contains(TermMode::APP_CURSOR),
            3 => self.mode.contains(TermMode::COLUMN_132),
            5 => self.mode.contains(TermMode::INVERSE),
            6 => self.mode.contains(TermMode::ORIGIN),
            7 => self.mode.contains(TermMode::AUTO_WRAP),
            9 => self.mode.contains(TermMode::MOUSE_X10),
            12 => self.mode.contains(TermMode::CURSOR_BLINK),
            25 => self.mode.contains(TermMode::SHOW_CURSOR),
            69 => self.mode.contains(TermMode::LEFT_RIGHT_MARGIN),
            1000 => self.mode.contains(TermMode::MOUSE_REPORT_CLICK),
            1002 => self.mode.contains(TermMode::MOUSE_DRAG),
            1003 => self.mode.contains(TermMode::MOUSE_MOTION),
            1004 => self.mode.contains(TermMode::FOCUS_IN_OUT),
            1006 => self.mode.contains(TermMode::MOUSE_SGR),
            2004 => self.mode.contains(TermMode::BRACKETED_PASTE),
            2026 => self.mode.contains(TermMode::SYNCHRONIZED_OUTPUT),
            _ => false,
        }
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
        let old_cols = self.cols;
        self.cols = new_cols;
        self.rows = new_rows;
        self.scroll_bottom = new_rows;
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(new_cols.saturating_sub(1));
        // Resize tab stops: preserve existing, add defaults for new columns
        if new_cols > old_cols {
            self.tab_stops.extend((old_cols..new_cols).map(|i| i % 8 == 0));
        } else {
            self.tab_stops.truncate(new_cols);
        }
        self.right_margin = self.right_margin.min(new_cols);
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
        let left = self.left_margin;
        let right = if self.mode.contains(TermMode::LEFT_RIGHT_MARGIN) {
            self.right_margin
        } else {
            self.cols
        };
        if left == 0 && right == self.cols {
            // Full width scroll - use fast whole-row operations
            if top == 0 && bottom == self.rows {
                self.grid.scroll_up();
            } else {
                self.grid.scroll_region_up(top, bottom);
            }
        } else {
            // Bounded scroll: shift cells within [left, right) column range
            for r in top..bottom.saturating_sub(1) {
                // Read source row, copy cells to destination
                let src: Vec<Cell> = if let Some(src_row) = self.grid.visible_row(r + 1) {
                    src_row[left..right].to_vec()
                } else {
                    continue;
                };
                if let Some(dst_row) = self.grid.visible_row_mut(r) {
                    for (i, cell) in src.into_iter().enumerate() {
                        dst_row[left + i] = cell;
                    }
                }
            }
            // Clear the bottom row within bounds
            if let Some(row) = self.grid.visible_row_mut(bottom - 1) {
                for c in left..right {
                    row[c].reset();
                }
            }
        }
    }

    fn scroll_region_down(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom > self.rows {
            return;
        }
        let left = self.left_margin;
        let right = if self.mode.contains(TermMode::LEFT_RIGHT_MARGIN) {
            self.right_margin
        } else {
            self.cols
        };
        if left == 0 && right == self.cols {
            // Full width scroll
            if top == 0 && bottom == self.rows {
                self.grid.scroll_down();
            } else {
                self.grid.scroll_region_down(top, bottom);
            }
        } else {
            // Bounded scroll: shift cells down within [left, right) column range
            for r in (top + 1..bottom).rev() {
                let src: Vec<Cell> = if let Some(src_row) = self.grid.visible_row(r - 1) {
                    src_row[left..right].to_vec()
                } else {
                    continue;
                };
                if let Some(dst_row) = self.grid.visible_row_mut(r) {
                    for (i, cell) in src.into_iter().enumerate() {
                        dst_row[left + i] = cell;
                    }
                }
            }
            // Clear the top row within bounds
            if let Some(row) = self.grid.visible_row_mut(top) {
                for c in left..right {
                    row[c].reset();
                }
            }
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

        // Handle wide character at last column:
        // Write a SpacerHead at current position, then wrap and write on next line
        if width == 2 && self.cursor.col >= self.cols - 1 {
            if let Some(cell) = self.grid.get_mut(self.cursor.row, self.cursor.col) {
                cell.c = ' ';
                cell.fg = self.fg;
                cell.bg = self.bg;
                cell.attrs = self.attrs.clone();
                cell.wide = WideMode::SpacerHead;
                cell.hyperlink = self.current_hyperlink.clone();
            }
            // Wrap to next line
            self.mark_row_wrapped();
            let bottom = self.scroll_bottom.min(self.rows);
            if self.cursor.row + 1 >= bottom {
                self.scroll_region_up(self.scroll_top, bottom);
            } else {
                self.cursor.row += 1;
            }
            self.cursor.col = 0;
        }

        // IRM: Insert mode shifts existing characters right
        if self.mode.contains(TermMode::INSERT) {
            if let Some(row) = self.grid.visible_row_mut(self.cursor.row) {
                let col = self.cursor.col;
                let shift = if width == 2 { 2 } else { 1 };
                for i in (col + shift..self.cols).rev() {
                    if i >= shift {
                        row[i] = row[i - shift].clone();
                    }
                }
                for i in col..(col + shift).min(self.cols) {
                    row[i].reset();
                }
            }
        }

        // Write the character
        if let Some(cell) = self.grid.get_mut(self.cursor.row, self.cursor.col) {
            cell.c = c;
            cell.fg = self.fg;
            cell.bg = self.bg;
            cell.attrs = self.attrs.clone();
            cell.wide = if width == 2 { WideMode::Wide } else { WideMode::Narrow };
            cell.protected = self.cell_protected;
            cell.hyperlink = self.current_hyperlink.clone();
        }

        // Write spacer for wide characters
        if width == 2 && self.cursor.col + 1 < self.cols {
            if let Some(cell) = self.grid.get_mut(self.cursor.row, self.cursor.col + 1) {
                cell.c = ' ';
                cell.fg = self.fg;
                cell.bg = self.bg;
                cell.attrs = self.attrs.clone();
                cell.wide = WideMode::SpacerTail;
                cell.hyperlink = self.current_hyperlink.clone();
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

    /// Reset tab stops to default (every 8 columns).
    fn reset_tabstops(&mut self) {
        for (i, stop) in self.tab_stops.iter_mut().enumerate() {
            *stop = i % 8 == 0;
        }
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
                    // If we landed on a spacer tail, skip over it to the wide char
                    if let Some(cell) = self.grid.get(self.cursor.row, self.cursor.col) {
                        if cell.wide == WideMode::SpacerTail && self.cursor.col > 0 {
                            self.cursor.col -= 1;
                        }
                    }
                }
                self.dirty = true;
            }
            0x09 => {
                // HT - Horizontal Tab
                let right_bound = if self.mode.contains(TermMode::LEFT_RIGHT_MARGIN) {
                    self.right_margin
                } else {
                    self.cols
                };
                let mut next = self.cursor.col + 1;
                while next < right_bound && !self.tab_stops.get(next).copied().unwrap_or(false) {
                    next += 1;
                }
                self.cursor.col = next.min(right_bound.saturating_sub(1));
                self.dirty = true;
            }
            0x0a | 0x0b | 0x0c => {
                // LF / VT / FF - Line Feed (VT and FF are treated as LF per VT spec)
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
            0x1a => {
                // SUB - Substitute: print '?' and move cursor
                self.put_char_at_cursor('?');
                self.last_char = Some('?');
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
            Some('q') if intermediates.contains(&0x2B) => {
                // XTGETTCAP - Request Terminal Capability (DCS + q ... ST)
                if let Ok(request) = String::from_utf8(data.clone()) {
                    let caps = request.split(';').filter_map(|cap| {
                        match cap {
                            "Ms" => Some("Ms=\x1b[<".to_string()),
                            "Ss" => Some("Ss=\x1b[1:2".to_string()),
                            "Se" => Some("Se=\x1b[1:0".to_string()),
                            "Sm" => Some("Sm=xterm".to_string()),
                            "Tk" => Some("Tk=pureterm".to_string()),
                            "TN" => Some("TN=pureterm".to_string()),
                            _ => None,
                        }
                    }).collect::<Vec<_>>();
                    if !caps.is_empty() {
                        // Response: DCS 0 + r cap=value ST
                        let resp = format!("\x1bP0+r{}\x1b\\", caps.join(";"));
                        self.respond(resp.as_bytes());
                    }
                }
            }
            Some('q') if intermediates.is_empty() || intermediates == [0x20] => {
                // Sixel image data (DCS ... q)
                let mut parser = crate::image::SixelParser::new();
                let image = parser.parse_all(&data);
                if image.width > 0 && image.height > 0 {
                    let id = self.image_manager.allocate_id();
                    let sixel_image = crate::image::SixelImage {
                        id,
                        width: image.width,
                        height: image.height,
                        data: image.data,
                        color_palette: image.color_palette,
                        cols: image.cols,
                        rows: image.rows,
                    };
                    self.image_manager.store_sixel(sixel_image);
                    debug!(
                        "Sixel image stored: id={}, size={}x{}, data={} bytes",
                        id, image.width, image.height, data.len()
                    );
                    self.dirty = true;
                } else {
                    debug!("Sixel DCS received: {} bytes (empty or invalid)", data.len());
                }
            }
            Some('G') if intermediates == [0x1F] || intermediates.is_empty() => {
                // APC with action 'G' is Kitty graphics (ESC _ G ... ST)
                // Parse and process the Kitty graphics command
                if let Some(cmd) = KittyGraphicsParser::parse_command(&data) {
                    self.handle_kitty_graphics(&cmd);
                } else {
                    debug!("Failed to parse Kitty graphics command");
                }
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
            "1" => {
                // Set icon name (treat same as title for now)
                if params.len() > 1 {
                    // Some terminals use icon name in the title bar
                    // We silently accept it
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
                // Pc: clipboard selector character(s): c=clipboard, p=primary, s=secondary
                // Pd: Base64 encoded content (or ? for query)
                if params.len() >= 2 {
                    let _clipboard_sel = String::from_utf8_lossy(params[1]);
                    let data: Vec<u8> = params.get(2).map(|p| p.to_vec()).unwrap_or_default();

                    // Check for query
                    if data.len() == 1 && data[0] == b'?' {
                        // Query clipboard - not supported, silently ignore
                    } else if !data.is_empty() {
                        // Write to clipboard
                        if let Some(ref mut cb) = self.clipboard_callback {
                            if let Ok(text) = base64_decode(&data) {
                                cb(text.as_str());
                            }
                        }
                    } else {
                        // Empty data = clear clipboard
                    }
                }
            }
            "4" => {
                // OSC 4 - Change/query dynamic colors
                // Format: OSC 4 ; c ; Pt ST  or  OSC 4 ; c ; ? ST (query)
                if params.len() >= 3 {
                    let idx = String::from_utf8_lossy(params[1]).parse::<usize>().unwrap_or(0);
                    if idx < 256 {
                        let color_spec = String::from_utf8_lossy(params[2]);
                        if color_spec == "?" {
                            // Query: respond with current color
                            let color = self.dynamic_colors[idx].unwrap_or(Color::Palette(idx as u8));
                            let resp = format!("\x1b]4;{};{}\x07", idx, format_osc_color_response(&color, (0xd0, 0xd0, 0xd0), (0x1e, 0x1e, 0x2e)));
                            self.respond(resp.as_bytes());
                        } else {
                            let color = parse_osc_color(params.get(2..).unwrap_or(&[]));
                            self.dynamic_colors[idx] = Some(color);
                            self.dirty = true;
                        }
                    }
                }
            }
            "10" => {
                // OSC 10 - Set/query foreground color
                if params.len() >= 2 {
                    let color_spec = String::from_utf8_lossy(params[1]);
                    if color_spec == "?" {
                        let resp = format!("\x1b]10;{}\x07", format_osc_color_response(&self.fg, (0xd0, 0xd0, 0xd0), (0x1e, 0x1e, 0x2e)));
                        self.respond(resp.as_bytes());
                    } else {
                        self.fg = parse_osc_color(params.get(1..).unwrap_or(&[]));
                        self.dirty = true;
                    }
                }
            }
            "11" => {
                // OSC 11 - Set/query background color
                if params.len() >= 2 {
                    let color_spec = String::from_utf8_lossy(params[1]);
                    if color_spec == "?" {
                        let resp = format!("\x1b]11;{}\x07", format_osc_color_response(&self.bg, (0xd0, 0xd0, 0xd0), (0x1e, 0x1e, 0x2e)));
                        self.respond(resp.as_bytes());
                    } else {
                        self.bg = parse_osc_color(params.get(1..).unwrap_or(&[]));
                        self.dirty = true;
                    }
                }
            }
            "12" => {
                // OSC 12 - Set/query cursor color
                if params.len() >= 2 {
                    let color_spec = String::from_utf8_lossy(params[1]);
                    if color_spec == "?" {
                        let cc = self.cursor_color.unwrap_or(Color::Named(NamedColor::Cursor));
                        let resp = format!("\x1b]12;{}\x07", format_osc_color_response(&cc, (0xd0, 0xd0, 0xd0), (0x1e, 0x1e, 0x2e)));
                        self.respond(resp.as_bytes());
                    } else {
                        self.cursor_color = Some(parse_osc_color(params.get(1..).unwrap_or(&[])));
                        self.dirty = true;
                    }
                }
            }
            "104" => {
                // OSC 104 - Reset color number(s) to default
                // Format: OSC 104 ; [Pc] ST  (no Pc = reset all)
                if params.len() < 2 {
                    // Reset all dynamic colors
                    self.dynamic_colors = [None; 256];
                } else {
                    // Reset specific color indices
                    for p in &params[1..] {
                        if let Ok(idx) = String::from_utf8_lossy(p).parse::<usize>() {
                            if idx < 256 {
                                self.dynamic_colors[idx] = None;
                            }
                        }
                    }
                }
                self.dirty = true;
            }
            "110" => {
                // OSC 110 - Reset foreground color to default
                self.fg = Color::Named(NamedColor::Foreground);
                self.dirty = true;
            }
            "111" => {
                // OSC 111 - Reset background color to default
                self.bg = Color::Named(NamedColor::Background);
                self.dirty = true;
            }
            "112" => {
                // OSC 112 - Reset cursor color to default
                self.cursor_color = None;
                self.dirty = true;
            }
            "9" => {
                // OSC 9 - Desktop notification (iTerm2 / Terminal.app extension)
                // Format: OSC 9 ; message ST
                // Silently accepted; UI layer can hook into bell_triggered or
                // a dedicated notification field if desired.
                log::trace!("OSC 9 notification received");
            }
            "21" => {
                // OSC 21 - Kitty color protocol (partial)
                // Format: OSC 21 ; key=value,key=value,... ST
                // Used by Kitty to set individual palette slots.
                // We silently accept for compatibility.
                log::trace!("OSC 21 Kitty color protocol received");
            }
            "22" => {
                // OSC 22 - Set mouse cursor shape (iTerm2 / Kitty)
                // Format: OSC 22 ; shape ST
                // Silently accepted; UI layer can use this to change cursor.
                log::trace!("OSC 22 cursor shape received");
            }
            "777" => {
                // OSC 777 - rxvt notification extension
                // Format: OSC 777 ; command ; title ; message ST
                // Silently accepted for compatibility.
                log::trace!("OSC 777 rxvt notification received");
            }
            "1337" => {
                // OSC 1337 - iTerm2 proprietary extensions
                // Format: OSC 1337 ; key=value ST
                // Common keys: SetMark, ShellIntegrationVersion, CurrentDir, RemoteHost
                // Silently accepted for compatibility.
                log::trace!("OSC 1337 iTerm2 extension received");
            }
            "5522" => {
                // OSC 5522 - Kitty clipboard protocol
                // Silently accepted for compatibility.
                log::trace!("OSC 5522 Kitty clipboard received");
            }
            "66" => {
                // OSC 66 - Kitty text sizing
                // Silently accepted for compatibility.
                log::trace!("OSC 66 Kitty text sizing received");
            }
            "3008" => {
                // OSC 3008 - Context signals
                // Silently accepted for compatibility.
                log::trace!("OSC 3008 context signal received");
            }
            "5" => {
                // OSC 5 - Change special color
                // Silently accepted for compatibility.
                log::trace!("OSC 5 received");
            }
            "6" => {
                // OSC 6 - Enable/disable Special Color Number c
                // Silently accepted for compatibility.
                log::trace!("OSC 6 received");
            }
            "17" => {
                // OSC 17 - Set highlight background color
                // Silently accepted for compatibility.
                log::trace!("OSC 17 received");
            }
            "19" => {
                // OSC 19 - Set highlight foreground color
                // Silently accepted for compatibility.
                log::trace!("OSC 19 received");
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
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                // Clamp to scroll region top if cursor is inside the scroll region
                let min_row = if self.cursor.row >= top && self.cursor.row < bottom {
                    top
                } else {
                    0
                };
                self.cursor.row = self.cursor.row.saturating_sub(n).max(min_row);
                self.pending_wrap = false;
            }
            'B' => {
                // CUD - Cursor Down
                let n = param_or(&params_vec, 0, 1) as usize;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                // Clamp to scroll region bottom if cursor is inside the scroll region
                let max_row = if self.cursor.row >= top && self.cursor.row < bottom {
                    bottom.saturating_sub(1)
                } else {
                    self.rows - 1
                };
                self.cursor.row = (self.cursor.row + n).min(max_row);
                self.pending_wrap = false;
            }
            'C' => {
                // CUF - Cursor Forward
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.col = (self.cursor.col + n).min(self.cols - 1);
                // Skip over spacer tail cells
                if let Some(cell) = self.grid.get(self.cursor.row, self.cursor.col) {
                    if cell.wide == WideMode::SpacerTail && self.cursor.col + 1 < self.cols {
                        self.cursor.col += 1;
                    }
                }
                self.pending_wrap = false;
            }
            'D' => {
                // CUB - Cursor Back
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.col = self.cursor.col.saturating_sub(n);
                // Skip over spacer tail cells
                if let Some(cell) = self.grid.get(self.cursor.row, self.cursor.col) {
                    if cell.wide == WideMode::SpacerTail && self.cursor.col > 0 {
                        self.cursor.col -= 1;
                    }
                }
                self.pending_wrap = false;
            }
            'H' | 'f' => {
                // CUP / HVP - Cursor Position
                let row = param_or(&params_vec, 0, 1).saturating_sub(1) as usize;
                let col = param_or(&params_vec, 1, 1).saturating_sub(1) as usize;
                if self.mode.contains(TermMode::ORIGIN) {
                    // Origin mode: row is relative to scroll region top
                    self.cursor.row = (row + self.scroll_top).min(self.scroll_bottom.saturating_sub(1));
                } else {
                    self.cursor.row = row.min(self.rows - 1);
                }
                // Respect left/right margins when DECLRMM is set
                let min_col = if self.mode.contains(TermMode::LEFT_RIGHT_MARGIN) {
                    self.left_margin
                } else {
                    0
                };
                let max_col = if self.mode.contains(TermMode::LEFT_RIGHT_MARGIN) {
                    self.right_margin.saturating_sub(1)
                } else {
                    self.cols - 1
                };
                self.cursor.col = (col + min_col).min(max_col);
                self.pending_wrap = false;
            }
            'J' => {
                // ED / DECSED - Erase Display
                // DECSED (CSI ? Ps J) always respects protected cells.
                // Standard ED (CSI Ps J) respects protection only in ISO mode.
                let selective = is_dec_private;
                let respect_protected = selective || self.protected_mode == ProtectedMode::Iso;
                let mode = param_or(&params_vec, 0, 0);
                match mode {
                    0 => {
                        // Erase from cursor to end
                        for c in self.cursor.col..self.cols {
                            if let Some(cell) = self.grid.get_mut(self.cursor.row, c) {
                                if !respect_protected || !cell.protected {
                                    cell.reset();
                                }
                            }
                        }
                        for r in (self.cursor.row + 1)..self.rows {
                            for c in 0..self.cols {
                                if let Some(cell) = self.grid.get_mut(r, c) {
                                    if !respect_protected || !cell.protected {
                                        cell.reset();
                                    }
                                }
                            }
                        }
                    }
                    1 => {
                        // Erase from start to cursor
                        for r in 0..self.cursor.row {
                            for c in 0..self.cols {
                                if let Some(cell) = self.grid.get_mut(r, c) {
                                    if !respect_protected || !cell.protected {
                                        cell.reset();
                                    }
                                }
                            }
                        }
                        for c in 0..=self.cursor.col {
                            if let Some(cell) = self.grid.get_mut(self.cursor.row, c) {
                                if !respect_protected || !cell.protected {
                                    cell.reset();
                                }
                            }
                        }
                    }
                    2 => {
                        // Erase entire display
                        if respect_protected {
                            for r in 0..self.rows {
                                for c in 0..self.cols {
                                    if let Some(cell) = self.grid.get_mut(r, c) {
                                        if !cell.protected {
                                            cell.reset();
                                        }
                                    }
                                }
                            }
                        } else {
                            self.grid.clear();
                        }
                    }
                    3 => {
                        // Erase scrollback history
                        self.grid.clear_history();
                    }
                    _ => {}
                }
                self.dirty = true;
            }
            'K' => {
                // EL / DECSEL - Erase Line
                // DECSEL (CSI ? Ps K) always respects protected cells.
                // Standard EL (CSI Ps K) respects protection only in ISO mode.
                let selective = is_dec_private;
                let respect_protected = selective || self.protected_mode == ProtectedMode::Iso;
                let mode = param_or(&params_vec, 0, 0);
                match mode {
                    0 => {
                        // Erase from cursor to end of line
                        for c in self.cursor.col..self.cols {
                            if let Some(cell) = self.grid.get_mut(self.cursor.row, c) {
                                if !respect_protected || !cell.protected {
                                    cell.reset();
                                }
                            }
                        }
                    }
                    1 => {
                        // Erase from start to cursor
                        for c in 0..=self.cursor.col {
                            if let Some(cell) = self.grid.get_mut(self.cursor.row, c) {
                                if !respect_protected || !cell.protected {
                                    cell.reset();
                                }
                            }
                        }
                    }
                    2 => {
                        // Erase entire line
                        if let Some(row) = self.grid.visible_row_mut(self.cursor.row) {
                            for cell in row.iter_mut() {
                                if !respect_protected || !cell.protected {
                                    cell.reset();
                                }
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
                // Only operate when cursor is within the scroll region
                if self.cursor.row >= top && self.cursor.row < bottom {
                    for _ in 0..n {
                        self.scroll_region_down(self.cursor.row, bottom);
                    }
                }
                self.dirty = true;
            }
            'M' => {
                // DL - Delete Lines
                let n = param_or(&params_vec, 0, 1) as usize;
                let top = self.scroll_top;
                let bottom = self.scroll_bottom.min(self.rows);
                // Only operate when cursor is within the scroll region
                if self.cursor.row >= top && self.cursor.row < bottom {
                    for _ in 0..n {
                        self.scroll_region_up(self.cursor.row, bottom);
                    }
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
                let respect_protected = self.protected_mode == ProtectedMode::Iso;
                for i in 0..n {
                    let col = self.cursor.col + i;
                    if col >= self.cols {
                        break;
                    }
                    if let Some(cell) = self.grid.get_mut(self.cursor.row, col) {
                        if !respect_protected || !cell.protected {
                            cell.reset();
                        }
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
                if is_dec_private {
                    // Kitty keyboard protocol: CSI ? Ps u
                    // Ps is a bitmask: bit 1=disambiguate, bit 2=report_events,
                    // bit 3=report_alternates, bit 4=report_all, bit 5=report_associated
                    let flags = param_or(&params_vec, 0, 0);
                    self.mode.set(TermMode::KITTY_KEYBOARD_DISAMBIGUATE, flags & 1 != 0);
                    self.mode.set(TermMode::KITTY_KEYBOARD_REPORT_EVENTS, flags & 2 != 0);
                    self.mode.set(TermMode::KITTY_KEYBOARD_REPORT_ALTERNATES, flags & 4 != 0);
                    self.mode.set(TermMode::KITTY_KEYBOARD_REPORT_ALL, flags & 8 != 0);
                    self.mode.set(TermMode::KITTY_KEYBOARD_REPORT_ASSOCIATED, flags & 16 != 0);
                    // Respond with current flags: CSI ? flags u
                    if let Some(ref mut writer) = self.response_writer {
                        let response = format!("\x1b[?{}u", flags);
                        writer(response.as_bytes());
                    }
                } else {
                    // Restore cursor position (CSI u or CSI Ps u)
                    if let Some(saved) = &self.saved_cursor {
                        self.cursor = saved.clone();
                    }
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
                if intermediates == [0x22] {
                    // DECSCA - Select Character Protection Attribute (CSI Ps " q)
                    // Ps=0 or 2: unprotected, Ps=1: protected (DEC mode)
                    let mode = param_or(&params_vec, 0, 0);
                    match mode {
                        1 => {
                            self.cell_protected = true;
                            self.protected_mode = ProtectedMode::Dec;
                        }
                        _ => {
                            self.cell_protected = false;
                        }
                    }
                } else {
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
                        7 => { self.cursor.style = CursorStyle::BlockHollow; self.cursor.blink = true; }
                        8 => { self.cursor.style = CursorStyle::BlockHollow; self.cursor.blink = false; }
                        _ => {}
                    }
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
                if intermediates == [0x3E] {
                    // DA2 - Secondary Device Attributes: CSI > Ps c
                    // Response: CSI > Pp; Pf; Hc c  (Pp=0=VT100, Pf=firmware, Hc=hardware)
                    self.respond(b"\x1b[>0;276;0c");
                } else if is_dec_private || intermediates.is_empty() {
                    // DA1 - Primary Device Attributes: CSI c or CSI ? c
                    // Response: CSI ? Pp; Pm c  (65=VT520, 22=ANSI color)
                    self.respond(b"\x1b[?65;22c");
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
                if intermediates == [0x24] && is_dec_private {
                    // DECRQM - Request Mode (CSI ? Ps $ p)
                    // Response: CSI ? Ps; Pm$ y  where Pm: 0=not recognized, 1=set, 2=reset, 3=permanently set, 4=permanently reset
                    let mode = param_or(&params_vec, 0, 0);
                    let value = if self.is_dec_mode_set(mode) { 1 } else { 2 };
                    let resp = format!("\x1b[?{};{}$y", mode, value);
                    self.respond(resp.as_bytes());
                } else if intermediates == [0x21] {
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
                    self.left_margin = 0;
                    self.right_margin = self.cols;
                    self.saved_cursor = None;
                    self.saved_attrs = None;
                    self.pending_wrap = false;
                    self.reset_tabstops();
                    // Reset character sets to default (G0 = ASCII)
                    self.charsets = [None, None, None, None];
                    self.gl_charset = CharsetIndex::G0;
                    self.gr_charset = CharsetIndex::G1;
                    self.shift_state = None;
                    // Reset dynamic colors
                    self.dynamic_colors = [None; 256];
                    self.cursor_color = None;
                    // Reset hyperlink
                    self.current_hyperlink = None;
                }
            }
            't' => {
                // XTWINOPS - Window Operations
                let mode = param_or(&params_vec, 0, 0);
                match mode {
                    1 => { /* De-iconify window - no-op for PureTerm */ }
                    2 => { /* Iconify window - no-op for PureTerm */ }
                    3 => { /* Move window to [x, y] - no-op */ }
                    4 => { /* Resize window to [height, width] pixels - no-op */ }
                    5 => { /* Raise window to front - no-op */ }
                    6 => { /* Resize window to [rows, cols] - no-op */ }
                    8 => {
                        // Resize text area to [rows, cols] characters
                        let rows = param_or(&params_vec, 1, self.rows as u16) as usize;
                        let cols = param_or(&params_vec, 2, self.cols as u16) as usize;
                        self.resize(cols, rows);
                    }
                    9 => { /* Maximize/restore window - no-op */ }
                    10 | 11 => { /* Maximize/restore vertical/horizontal - no-op */ }
                    18 => {
                        // Report text area size in characters
                        let resp = format!("\x1b[8;{};{}t", self.rows, self.cols);
                        self.respond(resp.as_bytes());
                    }
                    19 => {
                        // Report screen size in characters
                        let resp = format!("\x1b[9;{};{}t", self.rows, self.cols);
                        self.respond(resp.as_bytes());
                    }
                    20 => {
                        // Report iconification status (0=not iconified)
                        self.respond(b"\x1b[1;0t");
                    }
                    21 => {
                        // Report window title
                        let resp = format!("\x1b]l{}\x1b\\", self.title);
                        self.respond(resp.as_bytes());
                    }
                    22 => {
                        // Push title onto stack
                        self.title_stack.push(self.title.clone());
                    }
                    23 => {
                        // Pop title from stack
                        if let Some(popped) = self.title_stack.pop() {
                            self.title = popped;
                        }
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
                // DECCRA/DECERA/DECFRA - Rectangular area operations (CSI ... $ v/z/{)
                if intermediates == [0x24] {
                    match action {
                        'v' => {
                            // DECCRA - Copy Rectangular Area
                            let top = param_or(&params_vec, 0, 1).saturating_sub(1) as usize;
                            let left = param_or(&params_vec, 1, 1).saturating_sub(1) as usize;
                            let bottom = param_or(&params_vec, 2, self.rows as u16) as usize;
                            let right = param_or(&params_vec, 3, self.cols as u16) as usize;
                            let dest_row = param_or(&params_vec, 5, 1).saturating_sub(1) as usize;
                            let dest_col = param_or(&params_vec, 6, 1).saturating_sub(1) as usize;
                            let src_h = bottom.saturating_sub(top);
                            let src_w = right.saturating_sub(left);
                            let mut copied = vec![];
                            for dr in 0..src_h {
                                let mut row_data = vec![];
                                for dc in 0..src_w {
                                    let sr = top + dr; let sc = left + dc;
                                    if sr < self.rows && sc < self.cols {
                                        row_data.push(self.grid.get(sr, sc).cloned().unwrap_or_else(Cell::blank));
                                    }
                                }
                                copied.push(row_data);
                            }
                            for (dr, row_data) in copied.into_iter().enumerate() {
                                for (dc, cell) in row_data.into_iter().enumerate() {
                                    let tr = dest_row + dr; let tc = dest_col + dc;
                                    if tr < self.rows && tc < self.cols {
                                        if let Some(target) = self.grid.get_mut(tr, tc) { *target = cell; }
                                    }
                                }
                            }
                        }
                        'z' => {
                            // DECERA - Erase Rectangular Area
                            let top = param_or(&params_vec, 0, 1).saturating_sub(1) as usize;
                            let left = param_or(&params_vec, 1, 1).saturating_sub(1) as usize;
                            let bottom = param_or(&params_vec, 2, self.rows as u16) as usize;
                            let right = param_or(&params_vec, 3, self.cols as u16) as usize;
                            for row in top..bottom.min(self.rows) {
                                for col in left..right.min(self.cols) {
                                    if let Some(cell) = self.grid.get_mut(row, col) { cell.reset(); }
                                }
                            }
                        }
                        '{' => {
                            // DECFRA - Fill Rectangular Area
                            let fill_char = param_or(&params_vec, 0, 0) as u8 as char;
                            let top = param_or(&params_vec, 1, 1).saturating_sub(1) as usize;
                            let left = param_or(&params_vec, 2, 1).saturating_sub(1) as usize;
                            let bottom = param_or(&params_vec, 3, self.rows as u16) as usize;
                            let right = param_or(&params_vec, 4, self.cols as u16) as usize;
                            let c = if fill_char == '\0' { ' ' } else { fill_char };
                            for row in top..bottom.min(self.rows) {
                                for col in left..right.min(self.cols) {
                                    if let Some(cell) = self.grid.get_mut(row, col) {
                                        cell.c = c; cell.fg = self.fg; cell.bg = self.bg; cell.attrs = self.attrs;
                                    }
                                }
                            }
                        }
                        _ => debug!("Unhandled CSI ${} with {:?} params", action, params_vec),
                    }
                } else if intermediates == [0x2A] && action == 'x' {
                    // DECSACE - Select Attribute Change Extent
                } else if intermediates == [0x22] && action == 'p' {
                    // DECSCL - Set Conformance Level
                } else if intermediates == [0x3E] && action == 'q' {
                    // XTVERSION - CSI > Ps q
                    let resp = "\x1bP>|PureTerm 0.1\x1b\\";
                    self.respond(resp.as_bytes());
                } else {
                    debug!("Unhandled CSI {} with {:?} intermediates={:?}", action, params_vec, intermediates);
                }
            }
        }
        self.dirty = true;
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        // The vte crate delivers ESC sequences as follows:
        // - For ESC ( 0: intermediates=[0x28] (the '('), byte=0x30 (the '0')
        // - For ESC # 8: intermediates=[0x23] (the '#'), byte=0x38 (the '8')
        // - For ESC c: intermediates=[], byte=0x63
        //
        // Charset designation prefixes '(', ')', '*', '+' are intermediate bytes,
        // not the final byte. The final byte is the designation character.

        // Handle sequences with intermediates first
        if let Some(&intermediate) = intermediates.first() {
            match intermediate {
                b'(' => {
                    // ESC ( C - Designate G0 character set
                    self.charsets[0] = Some(charset_from_designation(byte));
                    self.dirty = true;
                    return;
                }
                b')' => {
                    // ESC ) C - Designate G1 character set
                    self.charsets[1] = Some(charset_from_designation(byte));
                    self.dirty = true;
                    return;
                }
                b'*' => {
                    // ESC * C - Designate G2 character set
                    self.charsets[2] = Some(charset_from_designation(byte));
                    self.dirty = true;
                    return;
                }
                b'+' => {
                    // ESC + C - Designate G3 character set
                    self.charsets[3] = Some(charset_from_designation(byte));
                    self.dirty = true;
                    return;
                }
                b'#' => {
                    match byte {
                        b'3' => {
                            // DECDHL - Double-Height Line, Top Half (ESC # 3)
                            self.grid.set_double_mode(self.cursor.row, crate::screen::DoubleMode::DoubleHeightTop);
                            self.dirty = true;
                        }
                        b'4' => {
                            // DECDHL - Double-Height Line, Bottom Half (ESC # 4)
                            self.grid.set_double_mode(self.cursor.row, crate::screen::DoubleMode::DoubleHeightBottom);
                            self.dirty = true;
                        }
                        b'5' => {
                            // DECSWL - Single-Width Line (ESC # 5)
                            self.grid.set_double_mode(self.cursor.row, crate::screen::DoubleMode::Normal);
                            self.dirty = true;
                        }
                        b'6' => {
                            // DECDWL - Double-Width Line (ESC # 6)
                            self.grid.set_double_mode(self.cursor.row, crate::screen::DoubleMode::DoubleWidth);
                            self.dirty = true;
                        }
                        b'8' => {
                            // DEC Screen Alignment Test (ESC # 8)
                            for row in 0..self.rows {
                                for col in 0..self.cols {
                                    if let Some(cell) = self.grid.get_mut(row, col) {
                                        cell.c = 'E';
                                    }
                                }
                            }
                            self.dirty = true;
                        }
                        _ => {
                            debug!("Unhandled ESC # sequence: ESC # {}", byte as char);
                        }
                    }
                    return;
                }
                _ => {
                    debug!(
                        "Unhandled ESC intermediate: 0x{:02x}, byte=0x{:02x}",
                        intermediate, byte
                    );
                    return;
                }
            }
        }

        // Handle sequences without intermediates (final byte only)
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
                self.reset_tabstops();
                self.pending_wrap = false;
                self.cell_protected = false;
                self.protected_mode = ProtectedMode::Off;
                self.charsets = [None, None, None, None];
                self.gl_charset = CharsetIndex::G0;
                self.gr_charset = CharsetIndex::G1;
                self.shift_state = None;
                self.left_margin = 0;
                self.right_margin = self.cols;
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
            b'H' => {
                // HTS - Horizontal Tab Set (set tab stop at current cursor column)
                if self.cursor.col < self.tab_stops.len() {
                    self.tab_stops[self.cursor.col] = true;
                }
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
            b'~' => {
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
            b'=' => {
                // DECKPAM - Keypad Application Mode
                self.mode.insert(TermMode::APP_KEYPAD);
            }
            b'>' => {
                // DECKPNM - Keypad Normal Mode
                self.mode.remove(TermMode::APP_KEYPAD);
            }
            b'V' => {
                // SPA - Start Protected Area (ISO protection)
                self.cell_protected = true;
                self.protected_mode = ProtectedMode::Iso;
            }
            b'W' => {
                // EPA - End Protected Area (ISO protection)
                self.cell_protected = false;
                // Note: protected_mode is NOT reset to Off per Ghostty behavior.
                // Once ISO mode is used, erase operations continue to respect protection.
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

/// Format a color for OSC query response in rgb:RR/GG/BB format.
fn format_osc_color_response(color: &Color, default_fg: (u8, u8, u8), default_bg: (u8, u8, u8)) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("rgb:{:02x}/{:02x}/{:02x}", r, g, b),
        Color::Palette(idx) => format!("rgb:{:02x}/{:02x}/{:02x}", idx, idx, idx), // simplified
        Color::Named(NamedColor::Foreground) => format!("rgb:{:02x}/{:02x}/{:02x}", default_fg.0, default_fg.1, default_fg.2),
        Color::Named(NamedColor::Background) => format!("rgb:{:02x}/{:02x}/{:02x}", default_bg.0, default_bg.1, default_bg.2),
        _ => format!("rgb:d0/d0/d0"),
    }
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
                4 => {
                    // Check for subparameter (4:0-5)
                    if param.len() > 1 {
                        match param[1] {
                            0 => self.attrs.underline = UnderlineStyle::None,
                            1 => self.attrs.underline = UnderlineStyle::Single,
                            2 => self.attrs.underline = UnderlineStyle::Double,
                            3 => self.attrs.underline = UnderlineStyle::Curly,
                            4 => self.attrs.underline = UnderlineStyle::Dotted,
                            5 => self.attrs.underline = UnderlineStyle::Dashed,
                            _ => self.attrs.underline = UnderlineStyle::Single,
                        }
                    } else {
                        self.attrs.underline = UnderlineStyle::Single;
                    }
                }
                5 => self.attrs.blink = true,
                6 => self.attrs.blink = true,
                7 => self.attrs.inverse = true,
                8 => self.attrs.hidden = true,
                9 => self.attrs.strikethrough = true,
                22 => { self.attrs.bold = false; self.attrs.dim = false; }
                23 => self.attrs.italic = false,
                24 => self.attrs.underline = UnderlineStyle::None,
                25 => self.attrs.blink = false,
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
                53 => self.attrs.overline = true,
                55 => self.attrs.overline = false,
                58 => {
                    // Underline color
                    if let Some(color) = parse_extended_color(param) {
                        self.attrs.underline_color = color;
                    }
                }
                59 => self.attrs.underline_color = Color::Named(NamedColor::Foreground),
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
                    9 => self.mode.insert(TermMode::MOUSE_X10),
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
                    1016 => self.mode.insert(TermMode::MOUSE_SGR_PIXELS),
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
                    2017 => self.mode.insert(TermMode::MODIFY_OTHER_KEYS),
                    2026 => {
                        // Synchronized output mode - begin batching
                        self.mode.insert(TermMode::SYNCHRONIZED_OUTPUT);
                    }
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
                    9 => self.mode.remove(TermMode::MOUSE_X10),
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
                    1016 => self.mode.remove(TermMode::MOUSE_SGR_PIXELS),
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
                    2017 => self.mode.remove(TermMode::MODIFY_OTHER_KEYS),
                    2026 => {
                        // Synchronized output mode - flush batch
                        self.mode.remove(TermMode::SYNCHRONIZED_OUTPUT);
                        self.dirty = true;
                    }
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
        // Underline style
        match self.attrs.underline {
            UnderlineStyle::None => {}
            UnderlineStyle::Single => sgr.push_str(";4:1"),
            UnderlineStyle::Double => sgr.push_str(";4:2"),
            UnderlineStyle::Curly => sgr.push_str(";4:3"),
            UnderlineStyle::Dotted => sgr.push_str(";4:4"),
            UnderlineStyle::Dashed => sgr.push_str(";4:5"),
        }
        // Blink
        if self.attrs.blink {
            sgr.push_str(";5");
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
        // Overline
        if self.attrs.overline {
            sgr.push_str(";53");
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

        // Underline color
        match self.attrs.underline_color {
            Color::Named(NamedColor::Foreground) => {}
            Color::Named(_) => {}
            Color::Palette(idx) => {
                sgr.push_str(&format!(";58;5;{}", idx));
            }
            Color::Rgb(r, g, b) => {
                sgr.push_str(&format!(";58;2;{};{};{}", r, g, b));
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
            CursorStyle::BlockHollow => match self.cursor.blink {
                true => "7",
                false => "8",
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

    /// Handle a Kitty graphics command.
    fn handle_kitty_graphics(&mut self, cmd: &crate::image::KittyGraphicsCommand) {
        use crate::image::{KittyAction, Placement};

        match cmd.action {
            KittyAction::Transmit => {
                // Check for chunked transfer
                if cmd.more_follows || cmd.offset.is_some() {
                    // Chunked transfer - store for later assembly
                    if let Some(image) = self.image_manager.process_chunked_transfer(cmd) {
                        debug!(
                            "Kitty image transmitted: id={}, size={}x{}",
                            image.id, image.width, image.height
                        );
                        self.dirty = true;
                    }
                } else {
                    // Direct transfer - decode and store immediately
                    if let Some(image) = self.image_manager.store_from_command(cmd) {
                        debug!(
                            "Kitty image transmitted: id={}, size={}x{}",
                            image.id, image.width, image.height
                        );
                        self.dirty = true;
                    }
                }
            }
            KittyAction::Place => {
                // Place an existing image on screen
                if let (Some(image_id), Some(placement_id)) = (cmd.image_id, cmd.placement_id) {
                    let placement = Placement {
                        image_id,
                        placement_id,
                        x: cmd.pos_x.unwrap_or(0) as u16,
                        y: cmd.pos_y.unwrap_or(0) as u16,
                        z: cmd.z_index.unwrap_or(0) as i16,
                        width: cmd.cell_cols.unwrap_or(1) as u16,
                        height: cmd.cell_rows.unwrap_or(1) as u16,
                    };
                    self.image_manager.add_placement(placement);

                    // Update image position if provided
                    self.image_manager.update_image_position(
                        image_id,
                        cmd.pos_x,
                        cmd.pos_y,
                        cmd.z_index,
                    );

                    debug!("Kitty image placed: image_id={}, placement_id={}", image_id, placement_id);
                    self.dirty = true;
                }
            }
            KittyAction::Delete => {
                if let Some(ids) = &cmd.delete_placements {
                    self.image_manager.delete_images(ids);
                } else if cmd.image_id.is_some() {
                    if let Some(id) = cmd.image_id {
                        self.image_manager.remove(id);
                    }
                } else {
                    // Delete all images
                    self.image_manager.delete_all();
                }
                debug!("Kitty images deleted");
                self.dirty = true;
            }
            KittyAction::Query => {
                // Kitty image query - respond with image info if found
                let image_id = cmd.image_id.unwrap_or(0);
                let placement_id = cmd.placement_id.unwrap_or(0);
                if let Some(img) = self.image_manager.get(image_id) {
                    // Image found: respond with dimensions
                    let resp = format!(
                        "\x1b_Gi={};I={};s={};v={};O=1\x1b\\",
                        image_id, placement_id, img.width, img.height
                    );
                    self.respond(resp.as_bytes());
                } else {
                    // Image not found: respond with OK (no data)
                    let resp = format!("\x1b_Gi={};I={};O=0\x1b\\", image_id, placement_id);
                    self.respond(resp.as_bytes());
                }
            }
            KittyAction::Frame => {
                // Frame control for animations - mark dirty for redraw
                self.dirty = true;
            }
        }
    }
}
