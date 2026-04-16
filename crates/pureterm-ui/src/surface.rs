//! Surface - single terminal instance
//!
//! Based on Ghostty's Surface.zig: the core terminal surface that wraps
//! a terminal instance with renderer, input handling, and UI state.

use std::time::Instant;

use pureterm_terminal::input::mods::Mods;
use pureterm_terminal::input::mouse::{MouseEvent, MouseEventType, MouseButton, MouseEncoding};
use pureterm_terminal::input::key::{KeyEvent, Key};
use pureterm_terminal::term::Terminal;

use crate::app::AppMessage;

/// Unique identifier for a surface
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub u64);

/// Surface - a single terminal instance
pub struct Surface {
    /// Unique identifier
    pub id: SurfaceId,
    /// Terminal state
    pub terminal: Terminal,
    /// Current size in cells
    pub size: SurfaceSize,
    /// Mouse state
    pub mouse: SurfaceMouse,
    /// Keyboard state
    pub keyboard: KeyboardState,
    /// Font size for this surface
    pub font_size: f32,
    /// Whether this surface is focused
    pub focused: bool,
    /// Whether this surface is read-only
    pub readonly: bool,
    /// Whether the child process has exited
    pub child_exited: bool,
    /// Whether selection scroll is active
    pub selection_scroll_active: bool,
    /// Mouse encoding mode
    pub mouse_encoding: MouseEncoding,
    /// Last render time
    pub last_render: Instant,
    /// Pending actions to dispatch
    pub pending_actions: Vec<Action>,
}

/// Surface size in cells and pixels
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceSize {
    /// Width in cells
    pub cols: usize,
    /// Height in cells
    pub rows: usize,
    /// Width in pixels
    pub pixel_width: u32,
    /// Height in pixels
    pub pixel_height: u32,
    /// Cell width in pixels
    pub cell_width: u32,
    /// Cell height in pixels
    pub cell_height: u32,
}

impl SurfaceSize {
    pub fn new(pixel_width: u32, pixel_height: u32, cell_width: u32, cell_height: u32) -> Self {
        let cols = if cell_width > 0 { pixel_width / cell_width } else { 80 } as usize;
        let rows = if cell_height > 0 { pixel_height / cell_height } else { 24 } as usize;
        Self {
            cols,
            rows,
            pixel_width,
            pixel_height,
            cell_width,
            cell_height,
        }
    }
}

impl Surface {
    /// Create a new surface with the given terminal
    pub fn new(id: SurfaceId, terminal: Terminal, size: SurfaceSize) -> Self {
        Self {
            id,
            terminal,
            size,
            mouse: SurfaceMouse::default(),
            keyboard: KeyboardState::default(),
            font_size: 14.0,
            focused: false,
            readonly: false,
            child_exited: false,
            selection_scroll_active: false,
            mouse_encoding: MouseEncoding::Sgr,
            last_render: Instant::now(),
            pending_actions: Vec::new(),
        }
    }

    /// Process a key event
    pub fn key_callback(&mut self, event: KeyEvent) {
        // If read-only, don't forward input
        if self.readonly {
            return;
        }

        // Record pressed key for tracking
        self.keyboard.pressed_key = Some(event.key);
        self.keyboard.last_binding_trigger = Instant::now();

        // Process through terminal input system
        // The terminal will encode and write the key
    }

    /// Process a mouse button event
    pub fn mouse_button_callback(
        &mut self,
        button: MouseButton,
        event_type: MouseEventType,
        row: usize,
        col: usize,
        mods: Mods,
    ) {
        let _event = MouseEvent {
            event: event_type,
            button,
            row,
            col,
            mods,
        };

        // Update mouse state
        self.mouse.last_row = row;
        self.mouse.last_col = col;
        self.mouse.last_button = Some(button);

        // Handle selection based on click
        match event_type {
            MouseEventType::Press => {
                match button {
                    MouseButton::Left => {
                        if mods.shift() {
                            // Extend selection
                        } else {
                            // Start new selection or clear
                        }
                    }
                    MouseButton::Middle => {
                        // Paste selection
                    }
                    MouseButton::Right => {
                        // Context menu or extend selection
                    }
                    _ => {}
                }
            }
            MouseEventType::Release => {
                self.mouse.dragging = false;
            }
            _ => {}
        }
    }

    /// Process mouse movement
    pub fn cursor_pos_callback(&mut self, row: usize, col: usize, mods: Mods) {
        let prev_row = self.mouse.last_row;
        let prev_col = self.mouse.last_col;

        self.mouse.last_row = row;
        self.mouse.last_col = col;

        // Update cursor shape based on position
        self.mouse.update_cursor_shape(mods);

        // If dragging, extend selection
        if self.mouse.dragging {
            // Extend selection to new position
            let _ = (prev_row, prev_col); // Track selection
        }
    }

    /// Process scroll event
    pub fn scroll_callback(&mut self, delta_rows: f64, mods: Mods) {
        // Handle scroll with modifiers
        if !mods.is_empty() {
            // Shift+scroll = horizontal scroll, etc.
        }

        // Scroll terminal
        let scroll_amount = (delta_rows.abs() as usize).max(1) * 3;
        if delta_rows > 0.0 {
            self.terminal.scroll_up(scroll_amount);
        } else {
            self.terminal.scroll_down(scroll_amount);
        }
    }

    /// Process focus change
    pub fn focus_callback(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Process size change
    pub fn size_callback(&mut self, new_size: SurfaceSize) {
        self.size = new_size;
        self.terminal.resize(new_size.cols, new_size.rows);
    }

    /// Process DPI/content scale change
    pub fn content_scale_callback(&mut self, scale: f32) {
        // Adjust rendering for new DPI
        let _ = scale;
    }

    /// Get the current mouse cursor shape
    pub fn cursor_shape(&self) -> CursorShape {
        self.mouse.cursor_shape
    }

    /// Take all pending actions
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.pending_actions)
    }

    /// Dispatch an action from this surface
    pub fn perform_action(&mut self, action: Action) {
        match action.target() {
            ActionTarget::Surface => {
                // Handle surface-level actions directly
                self.handle_surface_action(action);
            }
            ActionTarget::App => {
                // Queue for app to handle
                self.pending_actions.push(action);
            }
        }
    }

    fn handle_surface_action(&mut self, action: Action) {
        match action {
            Action::FontSizeDelta(delta) => {
                self.font_size = (self.font_size + delta).clamp(1.0, 256.0);
            }
            Action::FontSizeReset => {
                self.font_size = 14.0;
            }
            Action::CopySelection => {
                // Copy selection to clipboard
            }
            Action::PasteClipboard => {
                // Paste from clipboard
            }
            Action::ScrollToBottom => {
                self.terminal.scroll_to_bottom();
            }
            Action::ScrollToTop => {
                // Scroll to top of scrollback
            }
            Action::ToggleFullscreen => {
                self.pending_actions.push(Action::ToggleFullscreen);
            }
            _ => {}
        }
    }
}

// ── Mouse State ────────────────────────────────────────────────────────────────

/// Mouse cursor shape
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Text,     // I-beam
    Default,  // Arrow
    Pointer,  // Hand
    Crosshair,
    Hidden,
}

/// Mouse state for a surface
#[derive(Debug)]
pub struct SurfaceMouse {
    /// Last known row position
    pub last_row: usize,
    /// Last known column position
    pub last_col: usize,
    /// Currently pressed button
    pub last_button: Option<MouseButton>,
    /// Whether we're in a drag operation
    pub dragging: bool,
    /// Current cursor shape
    pub cursor_shape: CursorShape,
    /// Motion deduplication state
    pub motion_dedup: pureterm_terminal::input::mouse::MouseMotionDedup,
}

impl Default for SurfaceMouse {
    fn default() -> Self {
        Self {
            last_row: 0,
            last_col: 0,
            last_button: None,
            dragging: false,
            cursor_shape: CursorShape::Text,
            motion_dedup: pureterm_terminal::input::mouse::MouseMotionDedup::new(),
        }
    }
}

impl SurfaceMouse {
    /// Update cursor shape based on modifier state (state machine)
    pub fn update_cursor_shape(&mut self, mods: Mods) {
        // Cursor shape state machine (from Ghostty surface_mouse.zig):
        // Default: text (I-beam)
        // + Ctrl: default (arrow)
        // + Super: default
        // On link: pointer
        // During rectangle selection: crosshair
        // In mouse mode: default (don't change)
        // When hidden: don't show
        if self.cursor_shape == CursorShape::Hidden {
            return;
        }

        if mods.ctrl() || mods.super_() {
            self.cursor_shape = CursorShape::Default;
        } else if self.dragging {
            self.cursor_shape = CursorShape::Crosshair;
        } else {
            // Check if on a link (would need terminal state)
            self.cursor_shape = CursorShape::Text;
        }
    }
}

// ── Keyboard State ─────────────────────────────────────────────────────────────

/// Keyboard state for a surface
#[derive(Debug)]
pub struct KeyboardState {
    /// Currently pressed key
    pub pressed_key: Option<Key>,
    /// Time of last binding trigger
    pub last_binding_trigger: Instant,
    /// Whether IME preedit is active
    pub preedit_active: bool,
    /// Preedit text
    pub preedit_text: String,
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self {
            pressed_key: None,
            last_binding_trigger: Instant::now(),
            preedit_active: false,
            preedit_text: String::new(),
        }
    }
}

// ── Action System ──────────────────────────────────────────────────────────────

/// Actions that can be dispatched in the app
#[derive(Debug, Clone)]
pub enum Action {
    // Window management
    Quit,
    NewWindow,
    NewTab,
    CloseTab,
    NewSplit(SplitDirection),
    ToggleFullscreen,
    ToggleMaximize,
    ToggleQuickTerminal,

    // Tab management
    MoveTab(isize),
    GotoTab(usize),

    // Split management
    ResizeSplit(SplitResizeParameter),
    EqualizeSplits,
    MoveFocus(SplitDirection),

    // Font
    FontSizeDelta(f32),
    FontSizeReset,

    // Clipboard
    CopySelection,
    PasteClipboard,

    // Scroll
    ScrollToBottom,
    ScrollToTop,
    ScrollPageUp,
    ScrollPageDown,

    // View
    ToggleInspector,
    ToggleSearch,

    // Terminal
    ResetTerminal,
    ClearScrollback,

    // Config
    ReloadConfig,
}

/// Target for an action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionTarget {
    /// Action targets the app (window management, tabs, etc.)
    App,
    /// Action targets the current surface (font, clipboard, etc.)
    Surface,
}

impl Action {
    pub fn target(&self) -> ActionTarget {
        match self {
            Action::Quit
            | Action::NewWindow
            | Action::NewTab
            | Action::CloseTab
            | Action::NewSplit(_)
            | Action::ToggleFullscreen
            | Action::ToggleMaximize
            | Action::ToggleQuickTerminal
            | Action::MoveTab(_)
            | Action::GotoTab(_)
            | Action::ResizeSplit(_)
            | Action::EqualizeSplits
            | Action::MoveFocus(_)
            | Action::ReloadConfig
            | Action::ToggleInspector
            | Action::ToggleSearch => ActionTarget::App,

            Action::FontSizeDelta(_)
            | Action::FontSizeReset
            | Action::CopySelection
            | Action::PasteClipboard
            | Action::ScrollToBottom
            | Action::ScrollToTop
            | Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ResetTerminal
            | Action::ClearScrollback => ActionTarget::Surface,
        }
    }
}

/// Direction for split creation and focus movement
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Left,
    Right,
    Top,
    Bottom,
}

/// Parameters for split resize
#[derive(Debug, Clone, Copy)]
pub struct SplitResizeParameter {
    pub direction: SplitDirection,
    pub amount: usize,
}

// ── App-Level Action Handler ───────────────────────────────────────────────────

/// Handle app-level actions
pub fn handle_app_action(
    action: Action,
    app_mailbox: &mut Vec<AppMessage>,
) {
    match action {
        Action::Quit => {
            app_mailbox.push(AppMessage::Quit);
        }
        Action::NewWindow => {
            app_mailbox.push(AppMessage::NewWindow);
        }
        Action::NewTab => {
            app_mailbox.push(AppMessage::NewTab);
        }
        Action::CloseTab => {
            app_mailbox.push(AppMessage::CloseTab);
        }
        Action::NewSplit(dir) => {
            app_mailbox.push(AppMessage::NewSplit(dir));
        }
        Action::ToggleFullscreen => {
            app_mailbox.push(AppMessage::ToggleFullscreen);
        }
        Action::ToggleMaximize => {
            app_mailbox.push(AppMessage::ToggleMaximize);
        }
        Action::ToggleQuickTerminal => {
            app_mailbox.push(AppMessage::ToggleQuickTerminal);
        }
        Action::MoveTab(delta) => {
            app_mailbox.push(AppMessage::MoveTab(delta));
        }
        Action::GotoTab(idx) => {
            app_mailbox.push(AppMessage::GotoTab(idx));
        }
        Action::ResizeSplit(param) => {
            app_mailbox.push(AppMessage::ResizeSplit(param));
        }
        Action::EqualizeSplits => {
            app_mailbox.push(AppMessage::EqualizeSplits);
        }
        Action::MoveFocus(dir) => {
            app_mailbox.push(AppMessage::MoveFocus(dir));
        }
        Action::ReloadConfig => {
            app_mailbox.push(AppMessage::ReloadConfig);
        }
        Action::ToggleInspector => {
            app_mailbox.push(AppMessage::ToggleInspector);
        }
        Action::ToggleSearch => {
            app_mailbox.push(AppMessage::ToggleSearch);
        }
        _ => {
            // Surface-level actions are not handled here
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surface_size_calculation() {
        let size = SurfaceSize::new(1920, 1080, 10, 20);
        assert_eq!(size.cols, 192);
        assert_eq!(size.rows, 54);
    }

    #[test]
    fn test_mouse_cursor_shape_default() {
        let mouse = SurfaceMouse::default();
        assert_eq!(mouse.cursor_shape, CursorShape::Text);
    }

    #[test]
    fn test_mouse_cursor_shape_ctrl() {
        let mut mouse = SurfaceMouse::default();
        let mods = Mods::from_bits(Mods::CTRL);
        mouse.update_cursor_shape(mods);
        assert_eq!(mouse.cursor_shape, CursorShape::Default);
    }

    #[test]
    fn test_mouse_cursor_shape_dragging() {
        let mut mouse = SurfaceMouse::default();
        mouse.dragging = true;
        mouse.update_cursor_shape(Mods::empty());
        assert_eq!(mouse.cursor_shape, CursorShape::Crosshair);
    }

    #[test]
    fn test_mouse_cursor_shape_hidden_stays_hidden() {
        let mut mouse = SurfaceMouse::default();
        mouse.cursor_shape = CursorShape::Hidden;
        mouse.update_cursor_shape(Mods::empty());
        assert_eq!(mouse.cursor_shape, CursorShape::Hidden);
    }

    #[test]
    fn test_action_target() {
        assert_eq!(Action::Quit.target(), ActionTarget::App);
        assert_eq!(Action::FontSizeDelta(1.0).target(), ActionTarget::Surface);
        assert_eq!(Action::CopySelection.target(), ActionTarget::Surface);
        assert_eq!(Action::NewTab.target(), ActionTarget::App);
    }
}
