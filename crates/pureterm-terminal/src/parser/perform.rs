use crate::screen::{Cell, CellAttrs, Color, Cursor, CursorStyle, Grid, NamedColor, UnderlineStyle, WideMode};
use crate::mode::TermMode;
use crate::hyperlink::Hyperlink;
use crate::mark::{Mark, MarkKind};
use crate::charset::{Charset, CharsetIndex, map_char, charset_from_designation};
use crate::image::{ImageManager, KittyGraphicsParser};
use crate::block::BlockManager;
use log::{debug, trace};
use std::collections::VecDeque;

/// A row whose contents are complete (LF or wrap). Drained by Pane to feed triggers.
#[derive(Debug, Clone)]
pub struct CompletedLine {
    pub line_id: u64,
    pub text: String,
    pub cwd: Option<String>,
}

const COMPLETED_LINES_CAP: usize = 256;

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
    /// Notification callback for OSC 9 / OSC 777 desktop notifications.
    notification_callback: Option<Box<dyn FnMut(&str, &str) + Send>>,
    /// Mark callback for OSC 133 shell integration marks.
    mark_callback: Option<Box<dyn FnMut(crate::mark::MarkKind) + Send>>,
    /// Command block manager — builds CommandBlocks from OSC 133 marks.
    pub block_manager: BlockManager,
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
    /// Bounded queue of completed lines fed to Pane each frame.
    pub(crate) completed_lines: VecDeque<CompletedLine>,
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
            notification_callback: None,
            mark_callback: None,
            block_manager: BlockManager::new(),
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
            completed_lines: VecDeque::with_capacity(COMPLETED_LINES_CAP),
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

    /// Set the notification callback for OSC 9 / OSC 777 desktop notifications.
    /// The callback receives (title, body).
    pub fn set_notification_callback(&mut self, callback: Box<dyn FnMut(&str, &str) + Send>) {
        self.notification_callback = Some(callback);
    }

    /// Set the mark callback for OSC 133 shell integration marks.
    /// The callback receives the mark kind (PromptStart, CommandStart, OutputStart, OutputEnd).
    pub fn set_mark_callback(&mut self, callback: Box<dyn FnMut(crate::mark::MarkKind) + Send>) {
        self.mark_callback = Some(callback);
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
    pub fn restore_cursor_from_alt(&mut self) {
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
    pub fn switch_to_primary_buffer(&mut self) {
        if let Some(alt) = self.alt_buffer.take() {
            self.grid = alt.grid;
            // cursor and saved state are restored via restore_cursor_from_alt()
            // (called by mode handler after this)
            self.scroll_top = alt.scroll_top;
            self.scroll_bottom = alt.scroll_bottom;
        }
    }

    /// Check if currently on alternate screen buffer.
    pub fn is_alt_buffer(&self) -> bool {
        self.alt_buffer.is_some()
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
        // Collect rows that have PromptStart marks for block visual markers
        let block_markers: Vec<usize> = (0..rows.len())
            .filter(|&row| {
                self.grid.get_mark(row)
                    .map(|m| m.kind == crate::mark::MarkKind::PromptStart)
                    .unwrap_or(false)
            })
            .collect();
        ScreenSnapshot {
            rows,
            cursor_row: self.cursor.row,
            cursor_col: self.cursor.col,
            cursor_visible: self.cursor.visible && self.mode.contains(TermMode::SHOW_CURSOR),
            cursor_style: self.cursor.style,
            block_markers,
            ime_preedit: None,
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
        let wrapped_id = self.grid.line_id_for_visible(self.cursor.row);
        self.grid.set_wrapped(self.cursor.row, true);
        if let Some(id) = wrapped_id {
            self.enqueue_completed_by_id(id);
        }
    }

    /// Extract printable text of a row's cells, skipping wide spacers and
    /// trimming trailing spaces.
    fn extract_row_text_by_line_id(&self, id: u64) -> Option<String> {
        let loc = self.grid.find_row_by_line_id(id)?;
        let cells: &[crate::screen::Cell] = match loc {
            crate::screen::grid::RowLocation::Visible(r) => self.grid.visible_row(r)?,
            crate::screen::grid::RowLocation::History(idx) => self.grid.history_row(idx)?,
        };
        let mut s = String::with_capacity(cells.len());
        for c in cells {
            if c.wide == crate::screen::WideMode::SpacerTail
                || c.wide == crate::screen::WideMode::SpacerHead { continue; }
            s.push(c.c);
        }
        Some(s.trim_end_matches(' ').to_string())
    }

    /// Enqueue a completed line by id. Bounded; oldest dropped + warned.
    fn enqueue_completed_by_id(&mut self, id: u64) {
        let Some(text) = self.extract_row_text_by_line_id(id) else { return; };
        let cwd = self.current_dir.clone();
        if self.completed_lines.len() >= COMPLETED_LINES_CAP {
            log::warn!("triggers: completed_lines queue full ({}); dropping oldest", COMPLETED_LINES_CAP);
            self.completed_lines.pop_front();
        }
        self.completed_lines.push_back(CompletedLine { line_id: id, text, cwd });
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
    /// Rows that are command block starts (PromptStart marks).
    pub block_markers: Vec<usize>,
    /// IME preedit text to render at the cursor position.
    pub ime_preedit: Option<String>,
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
                // Snapshot id of the row about to be completed.
                let leaving_id = self.grid.line_id_for_visible(self.cursor.row);
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
                if let Some(id) = leaving_id {
                    self.enqueue_completed_by_id(id);
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
        if self.dcs_buffer.len() < 64 * 1024 * 1024 {
            self.dcs_buffer.push(byte);
        }
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
                if params.len() > 1 {
                    let message: String = params[1..]
                        .iter()
                        .flat_map(|p| p.iter().copied())
                        .map(|b| b as char)
                        .collect();
                    log::trace!("OSC 9 notification: {}", message);
                    if let Some(cb) = &mut self.notification_callback {
                        cb("Terminal", &message);
                    }
                }
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
                if params.len() > 2 {
                    let combined: Vec<u8> = params[1..]
                        .iter()
                        .flat_map(|p| p.iter().copied())
                        .collect();
                    let parts: Vec<&[u8]> = combined.split(|&b| b == b';').collect();
                    let title = if parts.len() > 1 {
                        String::from_utf8_lossy(parts[0]).to_string()
                    } else {
                        "Terminal".to_string()
                    };
                    let body = if parts.len() > 1 {
                        String::from_utf8_lossy(parts[1]).to_string()
                    } else {
                        String::from_utf8_lossy(&combined).to_string()
                    };
                    log::trace!("OSC 777 notification: {} - {}", title, body);
                    if let Some(cb) = &mut self.notification_callback {
                        cb(&title, &body);
                    }
                }
            }
            "1337" => {
                // OSC 1337 - iTerm2 proprietary extensions
                // Format: OSC 1337 ; File=name=<name>;size=<N>;inline=1:<base64-data> ST
                if params.len() > 1 {
                    let combined: Vec<u8> = params[1..].iter().flat_map(|p| p.iter().copied()).collect();
                    let rest = String::from_utf8_lossy(&combined);
                    if let Some(file_params) = rest.strip_prefix("File=") {
                        self.handle_iterm2_image(file_params);
                    } else {
                        log::trace!("OSC 1337 iTerm2 extension received (non-File)");
                    }
                }
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
                            let kind = mark.kind;
                            let row = self.cursor.row;
                            let exit_code = mark.exit_code;
                            let duration_micros = mark.duration_micros;
                            self.grid.set_mark(row, mark);
                            // Feed into the block manager
                            match kind {
                                MarkKind::PromptStart => {
                                    self.block_manager.on_prompt_start(row);
                                }
                                MarkKind::CommandStart => {
                                    self.block_manager.on_command_start(row);
                                }
                                MarkKind::OutputStart => {
                                    self.block_manager.on_output_start(row);
                                }
                                MarkKind::OutputEnd => {
                                    let duration_ms = duration_micros.map(|us| us / 1000);
                                    self.block_manager.on_output_end(row, exit_code, duration_ms);
                                }
                                MarkKind::User => {}
                            }
                            // Notify the mark callback
                            if let Some(ref mut cb) = self.mark_callback {
                                cb(kind);
                            }
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
        // Strip '/' separators to get raw hex digits
        let hex: String = rest.chars().filter(|c| *c != '/').collect();

        // 2 hex digits per channel (RRGGBB)
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        }

        // 3 hex digits per channel (RRRGGGBBB) — take upper byte
        if hex.len() == 9 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..3], 16),
                u8::from_str_radix(&hex[3..6], 16),
                u8::from_str_radix(&hex[6..9], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        }

        // 4 hex digits per channel (RRRRGGGGBBBB) — take upper byte
        if hex.len() == 12 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u16::from_str_radix(&hex[0..4], 16),
                u16::from_str_radix(&hex[4..8], 16),
                u16::from_str_radix(&hex[8..12], 16),
            ) {
                return Color::Rgb((r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8);
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

        let mut i = 0;
        while i < params.len() {
            let param = &params[i];
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
                    if param.len() > 1 {
                        // Subparameter syntax: 38:5:N or 38:2:R:G:B
                        if let Some(color) = parse_extended_color(param) {
                            self.fg = color;
                        }
                    } else if let Some((color, skip)) = parse_sgr_extended_color_lookahead(params, i) {
                        self.fg = color;
                        i += skip;
                    }
                }
                39 => self.fg = Color::Named(NamedColor::Foreground),
                40..=47 => self.bg = Color::Palette((code - 40) as u8),
                48 => {
                    // Extended background color
                    if param.len() > 1 {
                        if let Some(color) = parse_extended_color(param) {
                            self.bg = color;
                        }
                    } else if let Some((color, skip)) = parse_sgr_extended_color_lookahead(params, i) {
                        self.bg = color;
                        i += skip;
                    }
                }
                49 => self.bg = Color::Named(NamedColor::Background),
                53 => self.attrs.overline = true,
                55 => self.attrs.overline = false,
                58 => {
                    // Underline color
                    if param.len() > 1 {
                        if let Some(color) = parse_extended_color(param) {
                            self.attrs.underline_color = color;
                        }
                    } else if let Some((color, skip)) = parse_sgr_extended_color_lookahead(params, i) {
                        self.attrs.underline_color = color;
                        i += skip;
                    }
                }
                59 => self.attrs.underline_color = Color::Named(NamedColor::Foreground),
                90..=97 => self.fg = Color::Palette((code - 90 + 8) as u8),
                100..=107 => self.bg = Color::Palette((code - 100 + 8) as u8),
                _ => {}
            }
            i += 1;
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

    /// Handle an iTerm2 inline image (OSC 1337 ; File=...).
    ///
    /// Format: `File=name=<name>;size=<N>;inline=1;[width=W;][height=H;]:<base64-data>`
    ///
    /// Width/height values support three units:
    ///   - Plain number: cell count (e.g., `width=10` = 10 cells)
    ///   - Percent: percentage of terminal viewport (e.g., `width=50%`)
    ///   - Pixels: explicit pixel count (e.g., `width=100px`)
    ///
    /// The `name` parameter is base64-encoded in the iTerm2 protocol.
    /// The image data after the colon is base64-encoded and may be PNG, JPEG, GIF, etc.
    fn handle_iterm2_image(&mut self, params: &str) {
        use base64::Engine;

        // Split at the first ':' to separate params from base64 data
        let (meta, b64_data) = match params.split_once(':') {
            Some((m, d)) => (m, d),
            None => {
                log::warn!("iTerm2 image: missing base64 data separator ':'");
                return;
            }
        };

        // Parsed dimension with unit
        #[derive(Debug, Clone, Copy)]
        enum DimUnit {
            Cells(u32),
            Pixels(u32),
            Percent(u32),
        }

        fn parse_dimension(value: &str) -> Option<DimUnit> {
            if value.ends_with("px") {
                // Pixel units: "100px"
                value[..value.len() - 2].parse::<u32>().ok().map(DimUnit::Pixels)
            } else if value.ends_with('%') {
                // Percent units: "50%"
                value[..value.len() - 1].parse::<u32>().ok()
                    .map(|p| DimUnit::Percent(p.min(100)))
            } else if value.ends_with("auto") {
                None // Auto sizing — use natural image size
            } else {
                // Plain number = cell count
                value.parse::<u32>().ok().map(DimUnit::Cells)
            }
        }

        // Parse metadata key=value pairs
        let mut size = None;
        let mut inline = false;
        let mut width_dim: Option<DimUnit> = None;
        let mut height_dim: Option<DimUnit> = None;
        let mut preserve_aspect = true;

        for kv in meta.split(';') {
            if let Some((key, value)) = kv.split_once('=') {
                match key {
                    "name" => {
                        // iTerm2 encodes filename in base64
                        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(value) {
                            if let Ok(name) = String::from_utf8(decoded) {
                                // Extract just the filename component
                                let filename = std::path::Path::new(&name)
                                    .file_name()
                                    .and_then(|f| f.to_str())
                                    .unwrap_or(&name);
                                trace!("iTerm2 image: name={}", filename);
                            }
                        }
                    }
                    "size" => size = value.parse::<usize>().ok(),
                    "inline" => inline = value == "1",
                    "width" => width_dim = parse_dimension(value),
                    "height" => height_dim = parse_dimension(value),
                    "preserveAspectRatio" => preserve_aspect = value != "0",
                    _ => {}
                }
            }
        }

        if !inline {
            log::trace!("iTerm2 image: not inline, skipping");
            return;
        }

        // Decode base64 image data
        let raw_data = match base64::engine::general_purpose::STANDARD.decode(b64_data.trim()) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("iTerm2 image: base64 decode failed: {}", e);
                return;
            }
        };

        // Verify size if specified (warning only — don't abort)
        if let Some(expected) = size {
            if raw_data.len() != expected {
                log::warn!(
                    "iTerm2 image: size mismatch (expected {}, got {})",
                    expected,
                    raw_data.len()
                );
            }
        }

        // Decode image to RGBA pixels.
        // Try PNG (most common), then JPEG, then GIF via image crate if available.
        // For PNG we reuse the existing decode_png helper from image.rs.
        let (img_width, img_height, rgba_data) = if raw_data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            // PNG — reuse the decode_png function from crate::image which handles
            // all color types (RGBA, RGB, Grayscale, GrayscaleAlpha) properly.
            match crate::image::decode_png(&raw_data) {
                Some(result) => result,
                None => {
                    log::warn!("iTerm2 image: PNG decode failed");
                    return;
                }
            }
        } else if raw_data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            // JPEG — decode manually (no alpha channel in JPEG)
            match Self::decode_jpeg(&raw_data) {
                Some(result) => result,
                None => {
                    log::warn!("iTerm2 image: JPEG decode failed");
                    return;
                }
            }
        } else if raw_data.starts_with(&[0x47, 0x49, 0x46]) {
            // GIF — attempt decode
            match Self::decode_gif(&raw_data) {
                Some(result) => result,
                None => {
                    log::warn!("iTerm2 image: GIF decode failed");
                    return;
                }
            }
        } else if raw_data.starts_with(&[0x52, 0x49, 0x46, 0x46]) {
            // WebP (RIFF container) — not yet supported
            log::warn!("iTerm2 image: WebP format not supported yet ({} bytes)", raw_data.len());
            return;
        } else {
            log::warn!(
                "iTerm2 image: unrecognized image format (first bytes: {:02X?}, {} bytes)",
                &raw_data[..raw_data.len().min(8)],
                raw_data.len()
            );
            return;
        };

        // Calculate cell dimensions based on the parsed dimension units.
        // We use an approximate cell size of 8x16 pixels for pixel-based conversions.
        // The renderer can refine the exact placement later.
        let cell_w_px: f32 = 8.0;
        let cell_h_px: f32 = 16.0;
        let term_cols = self.cols as u32;
        let term_rows = self.rows as u32;

        let cols = match width_dim {
            Some(DimUnit::Cells(c)) => c.max(1),
            Some(DimUnit::Pixels(px)) => ((px as f32 / cell_w_px).ceil() as u32).max(1),
            Some(DimUnit::Percent(pct)) => {
                let px = (term_cols as f32 * cell_w_px * pct as f32 / 100.0) as u32;
                ((px as f32 / cell_w_px).ceil() as u32).max(1)
            }
            None => ((img_width as f32 / cell_w_px).ceil() as u32).max(1),
        };

        // When only one dimension is specified and preserveAspectRatio is true,
        // compute the other dimension to maintain aspect ratio.
        let rows = if preserve_aspect && (width_dim.is_some() != height_dim.is_some()) {
            let aspect = img_height as f32 / img_width as f32;
            (cols as f32 * aspect).ceil() as u32
        } else {
            match height_dim {
                Some(DimUnit::Cells(r)) => r.max(1),
                Some(DimUnit::Pixels(px)) => ((px as f32 / cell_h_px).ceil() as u32).max(1),
                Some(DimUnit::Percent(pct)) => {
                    let px = (term_rows as f32 * cell_h_px * pct as f32 / 100.0) as u32;
                    ((px as f32 / cell_h_px).ceil() as u32).max(1)
                }
                None => ((img_height as f32 / cell_h_px).ceil() as u32).max(1),
            }
        };

        // Store using ImageManager (reuse Kitty image storage)
        use crate::image::{TerminalImage, ImageFormat, Placement};
        let cursor_col = self.cursor.col as u32;
        let cursor_row = self.cursor.row as u32;
        let id = self.image_manager.allocate_id();

        let img = TerminalImage {
            id,
            width: img_width,
            height: img_height,
            data: rgba_data,
            cols,
            rows,
            col: cursor_col,
            row: cursor_row,
            format: ImageFormat::Png, // Original format was PNG/JPEG/GIF
            z_index: 0,
            has_alpha: true,
        };

        self.image_manager.store(img);

        let placement = Placement {
            image_id: id,
            placement_id: id,
            x: cursor_col as u16,
            y: cursor_row as u16,
            z: 0,
            width: cols as u16,
            height: rows as u16,
        };
        self.image_manager.add_placement(placement);

        // Advance cursor past the image (move down by number of rows)
        // iTerm2 protocol places the cursor after the last row of the image
        self.cursor.row = (self.cursor.row + rows as usize).min(self.rows - 1);

        debug!(
            "iTerm2 image placed: id={}, size={}x{}, cells={}x{}, pos=({},{})",
            id, img_width, img_height, cols, rows, cursor_col, cursor_row
        );
        self.dirty = true;
    }

    /// Decode JPEG image data to RGBA pixels.
    fn decode_jpeg(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
        // Minimal JPEG decoder: parse SOF0/SOF2 marker to get dimensions,
        // then use a simple Huffman decode. For a terminal emulator, a lightweight
        // approach is to use the `png` crate is not applicable here, so we
        // implement a basic JPEG baseline decoder.
        //
        // However, since most iTerm2 images are PNG and JPEG support is a nice-to-have,
        // we do a best-effort decode by extracting dimensions and converting the
        // raw MCU data. For production quality, a proper JPEG library should be used.
        // For now, we provide a functional implementation that handles common cases.

        // Parse JPEG markers to find SOF and extract dimensions
        let mut img_width = 0u32;
        let mut img_height = 0u32;

        // Skip JFIF header
        if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
            return None;
        }
        let mut pos = 2;

        while pos + 1 < data.len() {
            if data[pos] != 0xFF {
                pos += 1;
                continue;
            }
            let marker = data[pos + 1];
            pos += 2;

            match marker {
                0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                    // SOF marker — contains image dimensions
                    if pos + 6 > data.len() {
                        return None;
                    }
                    let _length = ((data[pos] as u16) << 8) | data[pos + 1] as u16;
                    let _precision = data[pos + 2];
                    img_height = ((data[pos + 3] as u32) << 8) | data[pos + 4] as u32;
                    img_width = ((data[pos + 5] as u32) << 8) | data[pos + 6] as u32;
                    break;
                }
                0xD0..=0xD7 => {
                    // RST markers — no length
                    continue;
                }
                0xD8 | 0xD9 | 0x01 => {
                    // SOI, EOI, TEM — no length
                    continue;
                }
                0xDA => {
                    // SOS — scan data starts, skip to find SOF
                    if pos + 2 > data.len() {
                        return None;
                    }
                    let length = ((data[pos] as usize) << 8) | data[pos + 1] as usize;
                    pos += length;
                    // Skip entropy-coded data until next marker
                    while pos + 1 < data.len() {
                        if data[pos] == 0xFF && data[pos + 1] != 0x00 && data[pos + 1] != 0xFF {
                            break;
                        }
                        pos += 1;
                    }
                }
                _ => {
                    // Marker with length — skip it
                    if pos + 2 > data.len() {
                        return None;
                    }
                    let length = ((data[pos] as usize) << 8) | data[pos + 1] as usize;
                    pos += length;
                }
            }
        }

        if img_width == 0 || img_height == 0 {
            return None;
        }

        // For a complete JPEG decode we would need a full IDCT implementation.
        // Since we don't want to add a heavy JPEG dependency, we create a placeholder
        // gray image and log a message. Users sending JPEG via iTerm2 should
        // ideally convert to PNG first (most tools do this automatically).
        log::warn!(
            "iTerm2 image: JPEG dimensions {}x{} detected but full JPEG decode requires an image library; \
             consider using PNG format instead",
            img_width, img_height
        );

        // Generate a placeholder gray checkerboard so the image area is visible
        let mut rgba = vec![0u8; img_width as usize * img_height as usize * 4];
        for y in 0..img_height as usize {
            for x in 0..img_width as usize {
                let offset = (y * img_width as usize + x) * 4;
                let gray = if (x / 8 + y / 8) % 2 == 0 { 200u8 } else { 160u8 };
                rgba[offset] = gray;
                rgba[offset + 1] = gray;
                rgba[offset + 2] = gray;
                rgba[offset + 3] = 255;
            }
        }

        Some((img_width, img_height, rgba))
    }

    /// Decode GIF image data to RGBA pixels (first frame only).
    fn decode_gif(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
        // Minimal GIF parser: extract dimensions from the logical screen descriptor
        if data.len() < 13 {
            return None;
        }
        // GIF header: "GIF89a" or "GIF87a"
        if &data[0..3] != b"GIF" {
            return None;
        }

        let img_width = ((data[7] as u32) << 8) | data[6] as u32;
        let img_height = ((data[9] as u32) << 8) | data[8] as u32;

        if img_width == 0 || img_height == 0 {
            return None;
        }

        // Full GIF decode requires LZW decompression and color table parsing.
        // For a terminal emulator, most iTerm2 images are PNG, so we provide
        // a placeholder and log a recommendation.
        log::warn!(
            "iTerm2 image: GIF dimensions {}x{} detected but full GIF decode requires an image library; \
             consider using PNG format instead",
            img_width, img_height
        );

        // Generate a placeholder gray checkerboard
        let mut rgba = vec![0u8; img_width as usize * img_height as usize * 4];
        for y in 0..img_height as usize {
            for x in 0..img_width as usize {
                let offset = (y * img_width as usize + x) * 4;
                let gray = if (x / 8 + y / 8) % 2 == 0 { 200u8 } else { 160u8 };
                rgba[offset] = gray;
                rgba[offset + 1] = gray;
                rgba[offset + 2] = gray;
                rgba[offset + 3] = 255;
            }
        }

        Some((img_width, img_height, rgba))
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

/// Parse SGR extended color with lookahead for semicolon-delimited params.
/// For `\e[38;5;196m`, params are `[[38],[5],[196]]` and `start` is the index of `[38]`.
/// Returns the parsed color and the number of extra params to skip (beyond the base +1).
fn parse_sgr_extended_color_lookahead(params: &[Vec<u16>], start: usize) -> Option<(Color, usize)> {
    if start + 1 >= params.len() {
        return None;
    }
    let type_code = params[start + 1].first().copied().unwrap_or(0);
    match type_code {
        5 => {
            // 256-color: 38;5;N
            if start + 2 < params.len() {
                let idx = params[start + 2].first().copied().unwrap_or(0);
                Some((Color::Palette(idx as u8), 2))
            } else {
                None
            }
        }
        2 => {
            // Truecolor: 38;2;R;G;B
            if start + 4 < params.len() {
                let r = params[start + 2].first().copied().unwrap_or(0);
                let g = params[start + 3].first().copied().unwrap_or(0);
                let b = params[start + 4].first().copied().unwrap_or(0);
                Some((Color::Rgb(r as u8, g as u8, b as u8), 4))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: drive bytes through the parser into a VtePerform.
    fn feed(p: &mut VtePerform, bytes: &[u8]) {
        let mut parser = crate::parser::VteParser::new();
        parser.parse(bytes, p);
    }

    #[test]
    fn lf_enqueues_completed_line() {
        let mut p = VtePerform::new(10, 3, 100);
        feed(&mut p, b"hello\n");
        let line = p.completed_lines.pop_front().expect("LF should enqueue");
        assert_eq!(line.text, "hello");
    }

    #[test]
    fn wrap_enqueues_completed_line() {
        let mut p = VtePerform::new(5, 3, 100);
        feed(&mut p, b"abcdefgh");
        let first = p.completed_lines.pop_front().expect("wrap should enqueue");
        assert_eq!(first.text, "abcde");
    }

    #[test]
    fn ed_does_not_enqueue() {
        let mut p = VtePerform::new(10, 3, 100);
        feed(&mut p, b"hello");
        // Issue ESC[2J — full screen erase.
        feed(&mut p, b"\x1b[2J");
        assert!(p.completed_lines.is_empty(), "erase should not enqueue");
    }

    #[test]
    fn queue_caps_at_256() {
        let mut p = VtePerform::new(2, 1, 1000);
        for _ in 0..400 { feed(&mut p, b"a\n"); }
        assert!(p.completed_lines.len() <= 256);
    }
}
