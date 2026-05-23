//! The display subsystem including window management, font rasterization, and
//! GPU drawing.

use std::cmp;
use std::collections::HashMap;
use std::fmt::{self, Formatter};
use std::mem;
use std::time::{Duration, Instant};

use log::{debug, info};
use parking_lot::MutexGuard;
use serde::{Deserialize, Serialize};
use winit::dpi::PhysicalSize;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::RawWindowHandle;
use winit::window::CursorIcon;

use crossfont::{FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size as FontSize, Slant, Style, Weight};
use unicode_width::UnicodeWidthChar;

use alacritty_terminal::event::{EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::Dimensions as TermDimensions;
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::selection::Selection;
use alacritty_terminal::term::{
    self, MIN_COLUMNS, MIN_SCREEN_LINES, Term, TermDamage, TermMode,
};
use alacritty_terminal::vte::ansi::NamedColor;

use crate::config::UiConfig;
use crate::config::font::{Font, FontDescription};
use crate::config::window::Dimensions;
#[cfg(not(windows))]
use crate::config::window::StartupMode;
use crate::display::bell::VisualBell;
use crate::display::color::{List, Rgb};
use crate::display::content::RenderableContent;
use crate::display::damage::DamageTracker;
use crate::display::hint::{HintMatch, HintState};
use crate::display::meter::Meter;
use crate::display::window::Window;
use crate::event::{Event, EventType, Mouse, SearchState};
use crate::message_bar::MessageBuffer;
use crate::renderer::glyph_atlas::WgpuGlyphAtlas;
use crate::renderer::render_thread::{FrameData, RenderThreadHandle};
use crate::renderer::vertex_builders::FrameBuilder;
use crate::renderer::font_fallback::FontFallbackRegistry;
use crate::scheduler::{Scheduler, TimerId, Topic};
use crate::string::{ShortenDirection, StrShortener};

pub mod color;
pub mod content;
pub mod cursor;
pub mod hint;
pub mod rect;
pub mod window;

mod bell;
mod damage;
mod meter;

/// Label for the forward terminal search bar.
const FORWARD_SEARCH_LABEL: &str = "Search: ";

/// Label for the backward terminal search bar.
const BACKWARD_SEARCH_LABEL: &str = "Backward Search: ";

/// The character used to shorten the visible text like uri preview or search regex.
const SHORTENER: char = '…';

#[derive(Debug)]
pub enum Error {
    /// Error with window management.
    Window(window::Error),

    /// Error dealing with fonts.
    Font(crossfont::Error),

    /// Wgpu renderer error.
    Render(String),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Window(err) => err.source(),
            Error::Font(err) => err.source(),
            Error::Render(_) => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::Window(err) => err.fmt(f),
            Error::Font(err) => err.fmt(f),
            Error::Render(err) => write!(f, "{err}"),
        }
    }
}

impl From<window::Error> for Error {
    fn from(val: window::Error) -> Self {
        Error::Window(val)
    }
}

impl From<crossfont::Error> for Error {
    fn from(val: crossfont::Error) -> Self {
        Error::Font(val)
    }
}

/// Terminal size info.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
pub struct SizeInfo<T = f32> {
    /// Terminal window width.
    width: T,

    /// Terminal window height.
    height: T,

    /// Width of individual cell.
    cell_width: T,

    /// Height of individual cell.
    cell_height: T,

    /// Horizontal window padding.
    padding_x: T,

    /// Vertical window padding.
    padding_y: T,

    /// Number of lines in the viewport.
    screen_lines: usize,

    /// Number of columns in the viewport.
    columns: usize,
}

impl From<SizeInfo<f32>> for SizeInfo<u32> {
    fn from(size_info: SizeInfo<f32>) -> Self {
        Self {
            width: size_info.width as u32,
            height: size_info.height as u32,
            cell_width: size_info.cell_width as u32,
            cell_height: size_info.cell_height as u32,
            padding_x: size_info.padding_x as u32,
            padding_y: size_info.padding_y as u32,
            screen_lines: size_info.screen_lines,
            columns: size_info.screen_lines,
        }
    }
}

impl From<SizeInfo<f32>> for WindowSize {
    fn from(size_info: SizeInfo<f32>) -> Self {
        Self {
            num_cols: size_info.columns() as u16,
            num_lines: size_info.screen_lines() as u16,
            cell_width: size_info.cell_width() as u16,
            cell_height: size_info.cell_height() as u16,
        }
    }
}

impl<T: Clone + Copy> SizeInfo<T> {
    #[inline]
    pub fn width(&self) -> T {
        self.width
    }

    #[inline]
    pub fn height(&self) -> T {
        self.height
    }

    #[inline]
    pub fn cell_width(&self) -> T {
        self.cell_width
    }

    #[inline]
    pub fn cell_height(&self) -> T {
        self.cell_height
    }

    #[inline]
    pub fn padding_x(&self) -> T {
        self.padding_x
    }

    #[inline]
    pub fn padding_y(&self) -> T {
        self.padding_y
    }
}

impl SizeInfo<f32> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        width: f32,
        height: f32,
        cell_width: f32,
        cell_height: f32,
        mut padding_x: f32,
        mut padding_y: f32,
        dynamic_padding: bool,
    ) -> SizeInfo {
        if dynamic_padding {
            padding_x = Self::dynamic_padding(padding_x.floor(), width, cell_width);
            padding_y = Self::dynamic_padding(padding_y.floor(), height, cell_height);
        }

        let lines = (height - 2. * padding_y) / cell_height;
        let screen_lines = cmp::max(lines as usize, MIN_SCREEN_LINES);

        let columns = (width - 2. * padding_x) / cell_width;
        let columns = cmp::max(columns as usize, MIN_COLUMNS);

        SizeInfo {
            width,
            height,
            cell_width,
            cell_height,
            padding_x: padding_x.floor(),
            padding_y: padding_y.floor(),
            screen_lines,
            columns,
        }
    }

    #[inline]
    pub fn reserve_lines(&mut self, count: usize) {
        self.screen_lines = cmp::max(self.screen_lines.saturating_sub(count), MIN_SCREEN_LINES);
    }

    /// Check if coordinates are inside the terminal grid.
    ///
    /// The padding, message bar or search are not counted as part of the grid.
    #[inline]
    pub fn contains_point(&self, x: usize, y: usize) -> bool {
        x <= (self.padding_x + self.columns as f32 * self.cell_width) as usize
            && x > self.padding_x as usize
            && y <= (self.padding_y + self.screen_lines as f32 * self.cell_height) as usize
            && y > self.padding_y as usize
    }

    /// Calculate padding to spread it evenly around the terminal content.
    #[inline]
    fn dynamic_padding(padding: f32, dimension: f32, cell_dimension: f32) -> f32 {
        padding + ((dimension - 2. * padding) % cell_dimension) / 2.
    }
}

impl TermDimensions for SizeInfo {
    #[inline]
    fn columns(&self) -> usize {
        self.columns
    }

    #[inline]
    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    #[inline]
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct DisplayUpdate {
    pub dirty: bool,

    dimensions: Option<PhysicalSize<u32>>,
    cursor_dirty: bool,
    font: Option<Font>,
}

impl DisplayUpdate {
    pub fn dimensions(&self) -> Option<PhysicalSize<u32>> {
        self.dimensions
    }

    pub fn font(&self) -> Option<&Font> {
        self.font.as_ref()
    }

    pub fn cursor_dirty(&self) -> bool {
        self.cursor_dirty
    }

    pub fn set_dimensions(&mut self, dimensions: PhysicalSize<u32>) {
        self.dimensions = Some(dimensions);
        self.dirty = true;
    }

    pub fn set_font(&mut self, font: Font) {
        self.font = Some(font);
        self.dirty = true;
    }

    pub fn set_cursor_dirty(&mut self) {
        self.cursor_dirty = true;
        self.dirty = true;
    }
}

/// The display wraps a window, font rasterizer, and GPU renderer.
pub struct Display {
    pub window: Window,

    pub size_info: SizeInfo,

    /// Hint highlighted by the mouse.
    pub highlighted_hint: Option<HintMatch>,
    /// Frames since hint highlight was created.
    highlighted_hint_age: usize,

    /// Hint highlighted by the vi mode cursor.
    pub vi_highlighted_hint: Option<HintMatch>,
    /// Frames since hint highlight was created.
    vi_highlighted_hint_age: usize,

    pub raw_window_handle: RawWindowHandle,

    /// UI cursor visibility for blinking.
    pub cursor_hidden: bool,

    pub visual_bell: VisualBell,

    /// Mapped RGB values for each terminal color.
    pub colors: List,

    /// State of the keyboard hints.
    pub hint_state: HintState,

    /// Unprocessed display updates.
    pub pending_update: DisplayUpdate,

    /// The renderer update that takes place only once before the actual rendering.
    pub pending_renderer_update: Option<RendererUpdate>,

    /// The ime on the given display.
    pub ime: Ime,

    /// The state of the timer for frame scheduling.
    pub frame_timer: FrameTimer,

    /// Damage tracker for the given display.
    pub damage_tracker: DamageTracker,

    /// Font size used by the window.
    pub font_size: FontSize,

    // Mouse point position when highlighting hints.
    hint_mouse_point: Option<Point>,

    // --- Renderer fields ---
    render_thread: RenderThreadHandle,
    glyph_atlas: WgpuGlyphAtlas,
    frame_builder: FrameBuilder,
    rasterizer: Rasterizer,
    font_key: FontKey,
    font_descent: f32,
    font_fallback: FontFallbackRegistry,

    meter: Meter,
}

impl Display {
    /// Create a new Display using the wgpu render thread.
    pub fn new(
        window: Window,
        render_thread: RenderThreadHandle,
        config: &UiConfig,
        _tabbed: bool,
    ) -> Result<Display, Error> {
        let raw_window_handle = window.raw_window_handle();

        let scale_factor = window.scale_factor as f32;
        let mut rasterizer = Rasterizer::new()?;

        let font_size = config.font.size().scale(scale_factor);
        debug!("Loading \"{}\" font", &config.font.normal().family);
        let font = config.font.clone().with_size(font_size);

        // Load the font into the rasterizer to compute metrics.
        let font_desc = make_font_desc(font.normal(), Slant::Normal, Weight::Normal);
        let font_key = rasterizer.load_font(&font_desc, font_size)?;
        let metrics = rasterizer.metrics(font_key, font_size)?;
        let (cell_width, cell_height) = compute_cell_size(config, &metrics);
        let font_descent = metrics.descent;

        // Resize the window to account for the user configured size.
        if let Some(dimensions) = config.window.dimensions() {
            let size = window_size(config, dimensions, cell_width, cell_height, scale_factor);
            window.request_inner_size(size);
        }

        let padding = config.window.padding(window.scale_factor as f32);
        let viewport_size = window.inner_size();

        // Create new size with at least one column and row.
        let size_info = SizeInfo::new(
            viewport_size.width as f32,
            viewport_size.height as f32,
            cell_width,
            cell_height,
            padding.0,
            padding.1,
            config.window.dynamic_padding && config.window.dimensions().is_none(),
        );

        info!("Cell size: {cell_width} x {cell_height}");
        info!("Padding: {} x {}", size_info.padding_x(), size_info.padding_y());
        info!("Width: {}, Height: {}", size_info.width(), size_info.height());

        // Notify render thread about initial size.
        render_thread.resize(viewport_size.width, viewport_size.height);

        // Disable shadows for transparent windows on macOS.
        #[cfg(target_os = "macos")]
        window.set_has_shadow(config.window_opacity() >= 1.0);

        let is_wayland = matches!(raw_window_handle, RawWindowHandle::Wayland(_));

        // Set resize increments for the newly created window.
        if config.window.resize_increments {
            window.set_resize_increments(PhysicalSize::new(cell_width, cell_height));
        }

        window.set_visible(true);

        // Always focus new windows, even if no Alacritty window is currently focused.
        #[cfg(target_os = "macos")]
        window.focus_window();

        #[allow(clippy::single_match)]
        #[cfg(not(windows))]
        if !_tabbed {
            match config.window.startup_mode {
                #[cfg(target_os = "macos")]
                StartupMode::SimpleFullscreen => window.set_simple_fullscreen(true),
                StartupMode::Maximized if !is_wayland => window.set_maximized(true),
                _ => (),
            }
        }

        let hint_state = HintState::new(config.hints.alphabet());

        let mut damage_tracker = DamageTracker::new(size_info.screen_lines(), size_info.columns());
        damage_tracker.debug = config.debug.highlight_damage;

        let glyph_atlas = WgpuGlyphAtlas::new();
        let frame_builder = FrameBuilder::new();
        let font_fallback = FontFallbackRegistry::new(config.font.fallback_families.clone());

        Ok(Self {
            render_thread,
            glyph_atlas,
            frame_builder,
            rasterizer,
            font_key,
            font_descent,
            font_fallback,
            visual_bell: VisualBell::from(&config.bell),
            colors: List::from(&config.colors),
            frame_timer: FrameTimer::new(),
            raw_window_handle,
            damage_tracker,
            hint_state,
            size_info,
            font_size,
            window,
            pending_renderer_update: Default::default(),
            vi_highlighted_hint_age: Default::default(),
            highlighted_hint_age: Default::default(),
            vi_highlighted_hint: Default::default(),
            highlighted_hint: Default::default(),
            hint_mouse_point: Default::default(),
            pending_update: Default::default(),
            cursor_hidden: Default::default(),
            meter: Default::default(),
            ime: Default::default(),
        })
    }

    /// Update font size and cell dimensions.
    ///
    /// This will return a tuple of the cell width and height.
    fn update_font_size(
        rasterizer: &mut Rasterizer,
        config: &UiConfig,
        font: &Font,
        font_size: FontSize,
    ) -> (f32, f32, FontKey, f32) {
        let font_desc = make_font_desc(font.normal(), Slant::Normal, Weight::Normal);
        let font_key = rasterizer.load_font(&font_desc, font_size)
            .expect("failed to load font after resize");
        let metrics = rasterizer.metrics(font_key, font_size)
            .expect("failed to get font metrics after resize");
        let (w, h) = compute_cell_size(config, &metrics);
        (w, h, font_key, metrics.descent)
    }

    /// Reset glyph atlas and fallback char cache.
    fn reset_glyph_cache(&mut self) {
        self.glyph_atlas = WgpuGlyphAtlas::new();
        self.font_fallback.clear_char_cache();
    }

    // XXX: this function must not call to any rendering related tasks. Renderer updates are
    // performed in [`Self::process_renderer_update`] right before drawing.
    //
    /// Process update events.
    pub fn handle_update<T>(
        &mut self,
        terminal: &mut Term<T>,
        pty_resize_handle: &mut dyn OnResize,
        message_buffer: &MessageBuffer,
        search_state: &mut SearchState,
        config: &UiConfig,
    ) where
        T: EventListener,
    {
        let pending_update = mem::take(&mut self.pending_update);

        let (mut cell_width, mut cell_height) =
            (self.size_info.cell_width(), self.size_info.cell_height());

        if pending_update.font().is_some() || pending_update.cursor_dirty() {
            let renderer_update = self.pending_renderer_update.get_or_insert(Default::default());
            renderer_update.clear_font_cache = true
        }

        // Update font size and cell dimensions.
        if let Some(font) = pending_update.font() {
            let cell_dimensions =
                Self::update_font_size(&mut self.rasterizer, config, font, self.font_size);
            cell_width = cell_dimensions.0;
            cell_height = cell_dimensions.1;
            self.font_key = cell_dimensions.2;
            self.font_descent = cell_dimensions.3;

            info!("Cell size: {cell_width} x {cell_height}");

            // Mark entire terminal as damaged since glyph size could change without cell size
            // changes.
            self.damage_tracker.frame().mark_fully_damaged();
        }

        let (mut width, mut height) = (self.size_info.width(), self.size_info.height());
        if let Some(dimensions) = pending_update.dimensions() {
            width = dimensions.width as f32;
            height = dimensions.height as f32;
        }

        let padding = config.window.padding(self.window.scale_factor as f32);

        let mut new_size = SizeInfo::new(
            width,
            height,
            cell_width,
            cell_height,
            padding.0,
            padding.1,
            config.window.dynamic_padding,
        );

        // Update number of column/lines in the viewport.
        let search_active = search_state.history_index.is_some();
        let message_bar_lines = message_buffer.message().map_or(0, |m| m.text(&new_size).len());
        let search_lines = usize::from(search_active);
        new_size.reserve_lines(message_bar_lines + search_lines);

        // Update resize increments.
        if config.window.resize_increments {
            self.window.set_resize_increments(PhysicalSize::new(cell_width, cell_height));
        }

        // Resize when terminal when its dimensions have changed.
        if self.size_info.screen_lines() != new_size.screen_lines
            || self.size_info.columns() != new_size.columns()
        {
            // Resize PTY.
            pty_resize_handle.on_resize(new_size.into());

            // Resize terminal.
            terminal.resize(new_size);

            // Resize damage tracking.
            self.damage_tracker.resize(new_size.screen_lines(), new_size.columns());
        }

        // Check if dimensions have changed.
        if new_size != self.size_info {
            // Queue renderer update.
            let renderer_update = self.pending_renderer_update.get_or_insert(Default::default());
            renderer_update.resize = true;

            // Clear focused search match.
            search_state.clear_focused_match();
        }
        self.size_info = new_size;
    }

    /// Update the state of the renderer.
    pub fn process_renderer_update(&mut self) {
        let renderer_update = match self.pending_renderer_update.take() {
            Some(renderer_update) => renderer_update,
            _ => return,
        };

        // Resize render thread surface.
        if renderer_update.resize {
            let width = self.size_info.width() as u32;
            let height = self.size_info.height() as u32;
            self.render_thread.resize(width, height);
        }

        if renderer_update.clear_font_cache {
            self.reset_glyph_cache();
        }

        info!("Padding: {} x {}", self.size_info.padding_x(), self.size_info.padding_y());
        info!("Width: {}, Height: {}", self.size_info.width(), self.size_info.height());
    }

    /// Draw the screen.
    ///
    /// A reference to Term whose state is being drawn must be provided.
    pub fn draw<T: EventListener>(
        &mut self,
        mut terminal: MutexGuard<'_, Term<T>>,
        scheduler: &mut Scheduler,
        _message_buffer: &MessageBuffer,
        config: &UiConfig,
        search_state: &mut SearchState,
    ) {
        let size_info = self.size_info;

        // Phase 1: Collect renderable cells from terminal.
        // RenderableContent borrows self mutably, so we extract all data
        // from it before using self again.
        let (cells, cursor, bg_rgb, selection_range, display_offset) = {
            let mut content =
                RenderableContent::new(config, self, &terminal, search_state);
            let sr = content.selection_range();
            let bg = content.color(NamedColor::Background as usize);
            let do_ = content.display_offset();

            // Collect cells into a Vec (consuming the iterator).
            let mut cells_vec = Vec::new();
            for cell in &mut content {
                cells_vec.push(cell);
            }

            // cursor() consumes content, releasing the mutable borrow on self.
            let cur = content.cursor();
            (cells_vec, cur, bg, sr, do_)
        };
        // Now self is no longer borrowed — we can use all fields freely.

        let cursor_point = terminal.grid().cursor.point;
        let total_lines = terminal.grid().total_lines();
        let vi_mode = terminal.mode().contains(TermMode::VI);
        let vi_cursor_point = if vi_mode { Some(terminal.vi_mode_cursor.point) } else { None };

        // Add damage from the terminal.
        match terminal.damage() {
            TermDamage::Full => self.damage_tracker.frame().mark_fully_damaged(),
            TermDamage::Partial(damaged_lines) => {
                for damage in damaged_lines {
                    self.damage_tracker.frame().damage_line(damage);
                }
            },
        }
        terminal.reset_damage();

        // Drop terminal as early as possible to free lock.
        drop(terminal);

        // Invalidate highlighted hints if grid has changed.
        self.validate_hint_highlights(display_offset);

        // Add damage from alacritty's UI elements overlapping terminal.
        let requires_full_damage = self.visual_bell.intensity() != 0.
            || self.hint_state.active()
            || search_state.regex().is_some();
        if requires_full_damage {
            self.damage_tracker.frame().mark_fully_damaged();
            self.damage_tracker.next_frame().mark_fully_damaged();
        }

        let vi_cursor_viewport_point =
            vi_cursor_point.and_then(|cursor| term::point_to_viewport(display_offset, cursor));
        self.damage_tracker.damage_vi_cursor(vi_cursor_viewport_point);
        self.damage_tracker.damage_selection(selection_range, display_offset);

        let full_redraw = true;

        let _has_highlighted_hint =
            self.highlighted_hint.is_some() || self.vi_highlighted_hint.is_some();

        // Phase 1.5: Prime glyph atlas — rasterize any uncached characters.
        let mut fallback_map: HashMap<char, usize> = HashMap::new();
        {
            for cell in &cells {
                if cell.character == ' ' {
                    continue;
                }
                // Try primary font first (atlas index 0).
                if self.glyph_atlas.get(0, cell.character).is_some() {
                    continue;
                }
                // Also check if already resolved via fallback.
                if let Some(&fb_idx) = fallback_map.get(&cell.character) {
                    if self.glyph_atlas.get(fb_idx, cell.character).is_some() {
                        continue;
                    }
                }
                let glyph_key = GlyphKey {
                    character: cell.character,
                    font_key: self.font_key,
                    size: self.font_size,
                };
                if let Ok(glyph) = self.rasterizer.get_glyph(glyph_key) {
                    self.glyph_atlas.insert(0, cell.character, &glyph);
                } else if let Some(fb_idx) = self.font_fallback.resolve(
                    cell.character,
                    &mut self.rasterizer,
                    self.font_size,
                ) {
                    let fb_key = self.font_fallback.font_key_for_index(fb_idx).unwrap();
                    let fb_glyph_key = GlyphKey {
                        character: cell.character,
                        font_key: fb_key,
                        size: self.font_size,
                    };
                    if let Ok(glyph) = self.rasterizer.get_glyph(fb_glyph_key) {
                        self.glyph_atlas.insert(fb_idx, cell.character, &glyph);
                        fallback_map.insert(cell.character, fb_idx);
                    }
                }
                // Zero-width characters.
                if let Some(ref extra) = cell.extra {
                    if let Some(ref zerowidth) = extra.zerowidth {
                        for &c in zerowidth {
                            if self.glyph_atlas.get(0, c).is_some() {
                                continue;
                            }
                            if let Some(&fb_idx) = fallback_map.get(&c) {
                                if self.glyph_atlas.get(fb_idx, c).is_some() {
                                    continue;
                                }
                            }
                            let gk = GlyphKey {
                                character: c,
                                font_key: self.font_key,
                                size: self.font_size,
                            };
                            if let Ok(glyph) = self.rasterizer.get_glyph(gk) {
                                self.glyph_atlas.insert(0, c, &glyph);
                            } else if let Some(fb_idx) = self.font_fallback.resolve(
                                c,
                                &mut self.rasterizer,
                                self.font_size,
                            ) {
                                let fb_key = self.font_fallback.font_key_for_index(fb_idx).unwrap();
                                let fb_gk = GlyphKey {
                                    character: c,
                                    font_key: fb_key,
                                    size: self.font_size,
                                };
                                if let Ok(glyph) = self.rasterizer.get_glyph(fb_gk) {
                                    self.glyph_atlas.insert(fb_idx, c, &glyph);
                                    fallback_map.insert(c, fb_idx);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Phase 2: Build frame vertex arrays from pre-collected cells.
        let build_result = self.frame_builder.build_from_cells(
            cells,
            &cursor,
            bg_rgb,
            &size_info,
            &mut self.glyph_atlas,
            self.font_descent,
            &fallback_map,
        );

        // Handle IME positioning and search bar rendering.
        let _ime_position = match search_state.regex() {
            Some(regex) => {
                let search_label = match search_state.direction() {
                    Direction::Right => FORWARD_SEARCH_LABEL,
                    Direction::Left => BACKWARD_SEARCH_LABEL,
                };
                let search_text = Self::format_search(regex, search_label, size_info.columns());
                let line = size_info.screen_lines();
                let column = Column(search_text.chars().count() - 1);
                Some(Point::new(line, column))
            },
            None => {
                let num_lines = self.size_info.screen_lines();
                match vi_cursor_viewport_point {
                    None => term::point_to_viewport(display_offset, cursor_point)
                        .filter(|point| point.line < num_lines),
                    point => point,
                }
            },
        };

        // Draw line indicator if needed.
        if let Some(vi_cursor_point) = vi_cursor_point {
            let line = (-vi_cursor_point.line.0 + size_info.bottommost_line().0) as usize;
            let obstructed_column = Some(vi_cursor_point)
                .filter(|point| point.line == -(display_offset as i32))
                .map(|point| point.column);
            self.draw_line_indicator(config, total_lines, obstructed_column, line);
        } else if search_state.regex().is_some() {
            self.draw_line_indicator(config, total_lines, None, display_offset);
        }

        // Notify winit that we're about to present.
        self.window.pre_present_notify();

        // Prepare atlas data for upload.
        let atlas_dirty = self.glyph_atlas.is_dirty();
        let (atlas_data, atlas_width, atlas_height) = self
            .glyph_atlas
            .take_dirty_data()
            .map(|(data, w, h)| (Some(data), w, h))
            .unwrap_or_else(|| (None, 0, 0));

        let color_atlas_dirty = self.glyph_atlas.is_color_dirty();
        let (color_atlas_data, color_atlas_width, color_atlas_height) = self
            .glyph_atlas
            .take_color_dirty_data()
            .map(|(data, w, h)| (Some(data), w, h))
            .unwrap_or_else(|| (None, 0, 0));

        // Package vertex data into FrameData and send to render thread.
        let frame_data = Box::new(FrameData {
            bg_color_verts: std::mem::take(&mut self.frame_builder.bg_color_verts),
            bg_color_idxs: std::mem::take(&mut self.frame_builder.bg_color_idxs),
            cell_bg_verts: std::mem::take(&mut self.frame_builder.cell_bg_verts),
            cell_bg_idxs: std::mem::take(&mut self.frame_builder.cell_bg_idxs),
            text_verts: std::mem::take(&mut self.frame_builder.text_verts),
            text_idxs: std::mem::take(&mut self.frame_builder.text_idxs),
            deco_verts: std::mem::take(&mut self.frame_builder.deco_verts),
            deco_idxs: std::mem::take(&mut self.frame_builder.deco_idxs),
            cursor_verts: std::mem::take(&mut self.frame_builder.cursor_verts),
            cursor_idxs: std::mem::take(&mut self.frame_builder.cursor_idxs),
            screen_uniforms: build_result.screen_uniforms,
            bg_color: build_result.bg_color,
            full_redraw,
            atlas_dirty,
            atlas_data,
            atlas_width,
            atlas_height,
            color_atlas_data,
            color_atlas_width,
            color_atlas_height,
            color_atlas_dirty,
        });

        self.render_thread.request_frame(frame_data);

        // XXX: Request the new frame after submitting to the render thread,
        // so the time to finish operations is accounted for in the timeout.
        if !matches!(self.raw_window_handle, RawWindowHandle::Wayland(_)) {
            self.request_frame(scheduler);
        }

        self.damage_tracker.swap_damage();
    }

    /// Update to a new configuration.
    pub fn update_config(&mut self, config: &UiConfig) {
        self.damage_tracker.debug = config.debug.highlight_damage;
        self.visual_bell.update_config(&config.bell);
        self.colors = List::from(&config.colors);
    }

    /// Update the mouse/vi mode cursor hint highlighting.
    ///
    /// This will return whether the highlighted hints changed.
    pub fn update_highlighted_hints<T>(
        &mut self,
        term: &Term<T>,
        config: &UiConfig,
        mouse: &Mouse,
        modifiers: ModifiersState,
    ) -> bool {
        // Update vi mode cursor hint.
        let vi_highlighted_hint = if term.mode().contains(TermMode::VI) {
            let mods = ModifiersState::all();
            let point = term.vi_mode_cursor.point;
            hint::highlighted_at(term, config, point, mods)
        } else {
            None
        };
        let mut dirty = vi_highlighted_hint != self.vi_highlighted_hint;
        self.vi_highlighted_hint = vi_highlighted_hint;
        self.vi_highlighted_hint_age = 0;

        // Force full redraw if the vi mode highlight was cleared.
        if dirty {
            self.damage_tracker.frame().mark_fully_damaged();
        }

        // Abort if mouse highlighting conditions are not met.
        if !self.window.mouse_visible()
            || !mouse.inside_text_area
            || !term.selection.as_ref().is_none_or(Selection::is_empty)
        {
            if self.highlighted_hint.take().is_some() {
                self.damage_tracker.frame().mark_fully_damaged();
                dirty = true;
            }
            return dirty;
        }

        // Find highlighted hint at mouse position.
        let point = mouse.point(&self.size_info, term.grid().display_offset());
        let highlighted_hint = hint::highlighted_at(term, config, point, modifiers);

        // Update cursor shape.
        if highlighted_hint.is_some() {
            // If mouse changed the line, we should update the hyperlink preview, since the
            // highlighted hint could be disrupted by the old preview.
            dirty = self.hint_mouse_point.is_some_and(|p| p.line != point.line);
            self.hint_mouse_point = Some(point);
            self.window.set_mouse_cursor(CursorIcon::Pointer);
        } else if self.highlighted_hint.is_some() {
            self.hint_mouse_point = None;
            if term.mode().intersects(TermMode::MOUSE_MODE) && !term.mode().contains(TermMode::VI) {
                self.window.set_mouse_cursor(CursorIcon::Default);
            } else {
                self.window.set_mouse_cursor(CursorIcon::Text);
            }
        }

        let mouse_highlight_dirty = self.highlighted_hint != highlighted_hint;
        dirty |= mouse_highlight_dirty;
        self.highlighted_hint = highlighted_hint;
        self.highlighted_hint_age = 0;

        // Force full redraw if the mouse cursor highlight was changed.
        if mouse_highlight_dirty {
            self.damage_tracker.frame().mark_fully_damaged();
        }

        dirty
    }

    /// Draw IME preview (just update IME position, no direct GL rendering).
    #[inline(never)]
    fn draw_ime_preview(
        &mut self,
        point: Point<usize>,
        _fg: Rgb,
        _bg: Rgb,
        _config: &UiConfig,
    ) {
        // In the wgpu path, IME text rendering is handled through the FrameBuilder.
        // Here we only update the IME popup position for the window system.
        self.window.update_ime_position(point, &self.size_info);
    }

    /// Format search regex to account for the cursor and fullwidth characters.
    fn format_search(search_regex: &str, search_label: &str, max_width: usize) -> String {
        let label_len = search_label.len();

        // Skip `search_regex` formatting if only label is visible.
        if label_len > max_width {
            return search_label[..max_width].to_owned();
        }

        // The search string consists of `search_label` + `search_regex` + `cursor`.
        let mut bar_text = String::from(search_label);
        bar_text.extend(StrShortener::new(
            search_regex,
            max_width.wrapping_sub(label_len + 1),
            ShortenDirection::Left,
            Some(SHORTENER),
        ));

        // Add place for cursor.
        bar_text.push(' ');

        bar_text
    }

    /// Draw preview for the currently highlighted `Hyperlink` (no-op, handled by
    /// FrameBuilder).
    #[inline(never)]
    fn draw_hyperlink_preview(
        &mut self,
        _config: &UiConfig,
        _cursor_point: Option<Point>,
        _display_offset: usize,
    ) {
        // Hyperlink preview rendering is handled through the FrameBuilder.
    }

    /// Draw current search regex (no-op, handled by FrameBuilder).
    #[inline(never)]
    fn draw_search(&mut self, _config: &UiConfig, _text: &str) {
        // Search bar rendering is handled through the FrameBuilder.
    }

    /// Draw render timer (no-op, handled by FrameBuilder).
    #[inline(never)]
    fn draw_render_timer(&mut self, _config: &UiConfig) {
        // Render timer overlay is handled through the FrameBuilder.
    }

    /// Draw an indicator for the position of a line in history (no-op).
    #[inline(never)]
    fn draw_line_indicator(
        &mut self,
        _config: &UiConfig,
        _total_lines: usize,
        _obstructed_column: Option<Column>,
        _line: usize,
    ) {
        // Line indicator rendering is handled through the FrameBuilder.
    }

    /// Check whether a hint highlight needs to be cleared.
    fn validate_hint_highlights(&mut self, display_offset: usize) {
        let frame = self.damage_tracker.frame();
        let hints = [
            (&mut self.highlighted_hint, &mut self.highlighted_hint_age, true),
            (&mut self.vi_highlighted_hint, &mut self.vi_highlighted_hint_age, false),
        ];

        let num_lines = self.size_info.screen_lines();
        for (hint, hint_age, reset_mouse) in hints {
            let (start, end) = match hint {
                Some(hint) => (*hint.bounds().start(), *hint.bounds().end()),
                None => continue,
            };

            // Ignore hints that were created this frame.
            *hint_age += 1;
            if *hint_age == 1 {
                continue;
            }

            // Convert hint bounds to viewport coordinates.
            let start = term::point_to_viewport(display_offset, start)
                .filter(|point| point.line < num_lines)
                .unwrap_or_default();
            let end = term::point_to_viewport(display_offset, end)
                .filter(|point| point.line < num_lines)
                .unwrap_or_else(|| Point::new(num_lines - 1, self.size_info.last_column()));

            // Clear invalidated hints.
            if frame.intersects(start, end) {
                if reset_mouse {
                    self.window.set_mouse_cursor(CursorIcon::Default);
                }
                frame.mark_fully_damaged();
                *hint = None;
            }
        }
    }

    /// Request a new frame for a window on Wayland.
    fn request_frame(&mut self, scheduler: &mut Scheduler) {
        // Mark that we've used a frame.
        self.window.has_frame = false;

        // Get the display vblank interval.
        let monitor_vblank_interval = 1_000_000.
            / self
                .window
                .current_monitor()
                .and_then(|monitor| monitor.refresh_rate_millihertz())
                .unwrap_or(60_000) as f64;

        // Now convert it to micro seconds.
        let monitor_vblank_interval =
            Duration::from_micros((1000. * monitor_vblank_interval) as u64);

        let swap_timeout = self.frame_timer.compute_timeout(monitor_vblank_interval);

        let window_id = self.window.id();
        let timer_id = TimerId::new(Topic::Frame, window_id);
        let event = Event::new(EventType::Frame, window_id);

        scheduler.schedule(event, swap_timeout, false, timer_id);
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        // Gracefully shut down the render thread.
        self.render_thread.shutdown();
    }
}

/// Input method state.
#[derive(Debug, Default)]
pub struct Ime {
    /// Whether the IME is enabled.
    enabled: bool,

    /// Current IME preedit.
    preedit: Option<Preedit>,
}

impl Ime {
    #[inline]
    pub fn set_enabled(&mut self, is_enabled: bool) {
        if is_enabled {
            self.enabled = is_enabled
        } else {
            // Clear state when disabling IME.
            *self = Default::default();
        }
    }

    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[inline]
    pub fn set_preedit(&mut self, preedit: Option<Preedit>) {
        self.preedit = preedit;
    }

    #[inline]
    pub fn preedit(&self) -> Option<&Preedit> {
        self.preedit.as_ref()
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    /// The preedit text.
    text: String,

    /// Byte offset for cursor start into the preedit text.
    ///
    /// `None` means that the cursor is invisible.
    cursor_byte_offset: Option<(usize, usize)>,

    /// The cursor offset from the end of the start of the preedit in char width.
    cursor_end_offset: Option<(usize, usize)>,
}

impl Preedit {
    pub fn new(text: String, cursor_byte_offset: Option<(usize, usize)>) -> Self {
        let cursor_end_offset = if let Some(byte_offset) = cursor_byte_offset {
            // Convert byte offset into char offset.
            let start_to_end_offset =
                text[byte_offset.0..].chars().fold(0, |acc, ch| acc + ch.width().unwrap_or(1));
            let end_to_end_offset =
                text[byte_offset.1..].chars().fold(0, |acc, ch| acc + ch.width().unwrap_or(1));

            Some((start_to_end_offset, end_to_end_offset))
        } else {
            None
        };

        Self { text, cursor_byte_offset, cursor_end_offset }
    }
}

/// Pending renderer updates.
///
/// All renderer updates are cached to be applied just before rendering, to avoid platform-specific
/// rendering issues.
#[derive(Debug, Default, Copy, Clone)]
pub struct RendererUpdate {
    /// Should resize the window.
    resize: bool,

    /// Clear font caches.
    clear_font_cache: bool,
}

/// The frame timer state.
pub struct FrameTimer {
    /// Base timestamp used to compute sync points.
    base: Instant,

    /// The last timestamp we synced to.
    last_synced_timestamp: Instant,

    /// The refresh rate we've used to compute sync timestamps.
    refresh_interval: Duration,
}

impl FrameTimer {
    pub fn new() -> Self {
        let now = Instant::now();
        Self { base: now, last_synced_timestamp: now, refresh_interval: Duration::ZERO }
    }

    /// Compute the delay that we should use to achieve the target frame
    /// rate.
    pub fn compute_timeout(&mut self, refresh_interval: Duration) -> Duration {
        let now = Instant::now();

        // Handle refresh rate change.
        if self.refresh_interval != refresh_interval {
            self.base = now;
            self.last_synced_timestamp = now;
            self.refresh_interval = refresh_interval;
            return refresh_interval;
        }

        let next_frame = self.last_synced_timestamp + self.refresh_interval;

        if next_frame < now {
            // Redraw immediately if we haven't drawn in over `refresh_interval` microseconds.
            let elapsed_micros = (now - self.base).as_micros() as u64;
            let refresh_micros = self.refresh_interval.as_micros() as u64;
            self.last_synced_timestamp =
                now - Duration::from_micros(elapsed_micros % refresh_micros);
            Duration::ZERO
        } else {
            // Redraw on the next `refresh_interval` clock tick.
            self.last_synced_timestamp = next_frame;
            next_frame - now
        }
    }
}

/// Calculate the cell dimensions based on font metrics.
///
/// This will return a tuple of the cell width and height.
#[inline]
fn compute_cell_size(config: &UiConfig, metrics: &crossfont::Metrics) -> (f32, f32) {
    let offset_x = f64::from(config.font.offset.x);
    let offset_y = f64::from(config.font.offset.y);
    (
        (metrics.average_advance + offset_x).floor().max(1.) as f32,
        (metrics.line_height + offset_y).floor().max(1.) as f32,
    )
}

/// Calculate the size of the window given padding, terminal dimensions and cell size.
fn window_size(
    config: &UiConfig,
    dimensions: Dimensions,
    cell_width: f32,
    cell_height: f32,
    scale_factor: f32,
) -> PhysicalSize<u32> {
    let padding = config.window.padding(scale_factor);

    let grid_width = cell_width * dimensions.columns.max(MIN_COLUMNS) as f32;
    let grid_height = cell_height * dimensions.lines.max(MIN_SCREEN_LINES) as f32;

    let width = (padding.0).mul_add(2., grid_width).floor();
    let height = (padding.1).mul_add(2., grid_height).floor();

    PhysicalSize::new(width as u32, height as u32)
}

/// Convert a config `FontDescription` to a crossfont `FontDesc`.
fn make_font_desc(desc: &FontDescription, slant: Slant, weight: Weight) -> FontDesc {
    let style = if let Some(ref spec) = desc.style {
        Style::Specific(spec.clone())
    } else {
        Style::Description { slant, weight }
    };
    FontDesc::new(desc.family.clone(), style)
}
