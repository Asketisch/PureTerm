use crate::screen::{Cell, CellAttrs, Color, Cursor, Grid, NamedColor};
use crate::mode::TermMode;
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
            mode: TermMode::default(),
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
        }
    }

    /// Resize the terminal.
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        self.grid.resize(new_cols, new_rows);
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
        // Remove the top line of the region and add blank line at bottom
        let _ = bottom; // Use grid scrolling
        self.grid.scroll_up();
    }

    fn put_char_at_cursor(&mut self, c: char) {
        let width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width == 0 {
            return;
        }

        if self.cursor.col >= self.cols {
            if self.mode.contains(TermMode::AUTO_WRAP) {
                self.cursor.row += 1;
                self.cursor.col = 0;
                if self.cursor.row >= self.rows {
                    self.grid.scroll_up();
                    self.cursor.row = self.rows - 1;
                }
            } else {
                self.cursor.col = self.cols - 1;
            }
        }

        if let Some(cell) = self.grid.get_mut(self.cursor.row, self.cursor.col) {
            cell.c = c;
            cell.fg = self.fg;
            cell.bg = self.bg;
            cell.attrs = self.attrs;
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
            // Don't wrap yet, wait for next char
        }
        self.dirty = true;
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
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => {
                // BS - Backspace
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
                let bottom = self.scroll_bottom.min(self.rows);
                if self.cursor.row + 1 >= bottom {
                    self.grid.scroll_up();
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
                self.cursor.col = 0;
                self.dirty = true;
            }
            _ => {
                trace!("Unhandled execute byte: 0x{:02x}", byte);
            }
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
    }

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}

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
            _ => {
                debug!("Unhandled OSC: {}", param0);
            }
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let params_vec: Vec<Vec<u16>> = params.iter().map(|p| p.to_vec()).collect();

        match action {
            'A' => {
                // CUU - Cursor Up
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.row = self.cursor.row.saturating_sub(n);
            }
            'B' => {
                // CUD - Cursor Down
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.row = (self.cursor.row + n).min(self.rows - 1);
            }
            'C' => {
                // CUF - Cursor Forward
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.col = (self.cursor.col + n).min(self.cols - 1);
            }
            'D' => {
                // CUB - Cursor Back
                let n = param_or(&params_vec, 0, 1) as usize;
                self.cursor.col = self.cursor.col.saturating_sub(n);
            }
            'H' | 'f' => {
                // CUP - Cursor Position
                let row = param_or(&params_vec, 0, 1).saturating_sub(1) as usize;
                let col = param_or(&params_vec, 1, 1).saturating_sub(1) as usize;
                self.cursor.row = row.min(self.rows - 1);
                self.cursor.col = col.min(self.cols - 1);
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
                for _ in 0..n {
                    self.grid.scroll_down();
                }
                self.dirty = true;
            }
            'M' => {
                // DL - Delete Lines
                let n = param_or(&params_vec, 0, 1) as usize;
                for _ in 0..n {
                    self.grid.scroll_up();
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
                for _ in 0..n {
                    self.grid.scroll_up();
                }
                self.dirty = true;
            }
            'T' => {
                // SD - Scroll Down
                let n = param_or(&params_vec, 0, 1) as usize;
                for _ in 0..n {
                    self.grid.scroll_down();
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
                self.handle_set_mode(&params_vec);
            }
            'l' => {
                // RM - Reset Mode
                self.handle_reset_mode(&params_vec);
            }
            's' => {
                // Save cursor position
                self.saved_cursor = Some(self.cursor.clone());
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
            }
            'd' => {
                // VPA - Vertical Position Absolute
                let row = param_or(&params_vec, 0, 1).saturating_sub(1) as usize;
                self.cursor.row = row.min(self.rows - 1);
            }
            _ => {
                debug!("Unhandled CSI {} with {:?} params", action, params_vec);
            }
        }
        self.dirty = true;
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
    }
}

fn param_or(params: &[Vec<u16>], index: usize, default: u16) -> u16 {
    params.get(index).and_then(|p| p.first()).copied().unwrap_or(default)
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

    fn handle_set_mode(&mut self, params: &[Vec<u16>]) {
        for param in params {
            if let Some(&mode) = param.first() {
                match mode {
                    1 => self.mode.insert(TermMode::APP_CURSOR),
                    6 => self.mode.insert(TermMode::ORIGIN),
                    7 => self.mode.insert(TermMode::AUTO_WRAP),
                    25 => {
                        self.cursor.visible = true;
                        self.mode.insert(TermMode::SHOW_CURSOR);
                    }
                    1000 => self.mode.insert(TermMode::MOUSE_REPORT_CLICK),
                    1002 => self.mode.insert(TermMode::MOUSE_DRAG),
                    1003 => self.mode.insert(TermMode::MOUSE_MOTION),
                    1006 => self.mode.insert(TermMode::MOUSE_SGR),
                    2004 => self.mode.insert(TermMode::BRACKETED_PASTE),
                    1005 | 1015 => {} // Unsupported mouse modes
                    _ => debug!("Unhandled set mode: {}", mode),
                }
            }
        }
    }

    fn handle_reset_mode(&mut self, params: &[Vec<u16>]) {
        for param in params {
            if let Some(&mode) = param.first() {
                match mode {
                    1 => self.mode.remove(TermMode::APP_CURSOR),
                    6 => self.mode.remove(TermMode::ORIGIN),
                    7 => self.mode.remove(TermMode::AUTO_WRAP),
                    25 => {
                        self.cursor.visible = false;
                        self.mode.remove(TermMode::SHOW_CURSOR);
                    }
                    1000 => self.mode.remove(TermMode::MOUSE_REPORT_CLICK),
                    1002 => self.mode.remove(TermMode::MOUSE_DRAG),
                    1003 => self.mode.remove(TermMode::MOUSE_MOTION),
                    1006 => self.mode.remove(TermMode::MOUSE_SGR),
                    2004 => self.mode.remove(TermMode::BRACKETED_PASTE),
                    _ => debug!("Unhandled reset mode: {}", mode),
                }
            }
        }
    }
}

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
