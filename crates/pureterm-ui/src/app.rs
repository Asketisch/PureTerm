//! App - Application runtime
//!
//! Based on Ghostty's App.zig: manages all surfaces, shared font grid,
//! message routing, and configuration.

use pureterm_core::config::AppConfig;
use pureterm_core::hotkey::HotkeyService;

use crate::surface::{SurfaceId, SplitDirection, SplitResizeParameter};
use crate::tabs::TabManager;

/// App-level messages (mailbox pattern from Ghostty)
#[derive(Debug, Clone)]
pub enum AppMessage {
    Quit,
    NewWindow,
    NewTab,
    CloseTab,
    NewSplit(SplitDirection),
    ToggleFullscreen,
    ToggleMaximize,
    ToggleQuickTerminal,
    MoveTab(isize),
    GotoTab(usize),
    ResizeSplit(SplitResizeParameter),
    EqualizeSplits,
    MoveFocus(SplitDirection),
    ReloadConfig,
    ToggleInspector,
    ToggleSearch,
    ConfigChanged(Box<AppConfig>),
    SurfaceDestroyed(SurfaceId),
}

/// Main application state.
pub struct App {
    pub config: AppConfig,
    pub tab_manager: TabManager,
    pub hotkey_service: HotkeyService,
    pub show_settings: bool,
    pub search_visible: bool,
    pub search_query: String,
    pub quitting: bool,

    // ── Ghostty-style App fields ──────────────────────────────────────────
    /// All active surface IDs
    pub surfaces: Vec<SurfaceId>,
    /// Currently focused surface
    pub focused_surface: Option<SurfaceId>,
    /// Application-level mailbox for messages
    pub mailbox: Vec<AppMessage>,
    /// Next surface ID
    next_surface_id: u64,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let hotkey_service = HotkeyService::new(&config.hotkeys);
        Self {
            config,
            tab_manager: TabManager::new(),
            hotkey_service,
            show_settings: false,
            search_visible: false,
            search_query: String::new(),
            quitting: false,
            surfaces: Vec::new(),
            focused_surface: None,
            mailbox: Vec::new(),
            next_surface_id: 1,
        }
    }

    /// Generate a unique surface ID
    pub fn allocate_surface_id(&mut self) -> SurfaceId {
        let id = self.next_surface_id;
        self.next_surface_id += 1;
        SurfaceId(id)
    }

    /// Register a new surface
    pub fn add_surface(&mut self, id: SurfaceId) {
        if !self.surfaces.contains(&id) {
            self.surfaces.push(id);
        }
        self.focused_surface = Some(id);
    }

    /// Remove a surface
    pub fn remove_surface(&mut self, id: SurfaceId) {
        self.surfaces.retain(|s| *s != id);
        if self.focused_surface == Some(id) {
            self.focused_surface = self.surfaces.last().copied();
        }
    }

    /// Process all pending messages in the mailbox
    pub fn process_messages(&mut self) {
        let messages: Vec<AppMessage> = self.mailbox.drain(..).collect();
        for msg in messages {
            self.handle_message(msg);
        }
    }

    /// Handle a single app message
    fn handle_message(&mut self, msg: AppMessage) {
        match msg {
            AppMessage::Quit => {
                self.quitting = true;
            }
            AppMessage::NewTab => {
                // Handled by main loop
            }
            AppMessage::CloseTab => {
                self.tab_manager.close_active_tab();
            }
            AppMessage::ReloadConfig => {
                // Config reload handled by main loop
            }
            AppMessage::ToggleSearch => {
                self.search_visible = !self.search_visible;
            }
            AppMessage::ConfigChanged(config) => {
                self.config = *config;
                self.hotkey_service = HotkeyService::new(&self.config.hotkeys);
            }
            AppMessage::SurfaceDestroyed(id) => {
                self.remove_surface(id);
            }
            _ => {
                // Other messages handled by main loop
            }
        }
    }

    /// Process a key event and return if consumed.
    pub fn handle_key(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool, super_key: bool) -> bool {
        let modifiers = pureterm_core::hotkey::Modifiers {
            ctrl,
            alt,
            shift,
            super_key,
        };

        if self.hotkey_service.matches("tab_new", key, modifiers) {
            return true;
        }
        if self.hotkey_service.matches("tab_close", key, modifiers) {
            self.tab_manager.close_active_tab();
            return true;
        }
        if self.hotkey_service.matches("search", key, modifiers) {
            self.search_visible = !self.search_visible;
            return true;
        }
        if self.hotkey_service.matches("prev_tab", key, modifiers) {
            self.tab_manager.previous_tab();
            return true;
        }
        if self.hotkey_service.matches("next_tab", key, modifiers) {
            self.tab_manager.next_tab();
            return true;
        }

        false
    }

    /// Toggle settings panel.
    pub fn toggle_settings(&mut self) {
        self.show_settings = !self.show_settings;
    }

    /// Quit the application.
    pub fn quit(&mut self) {
        self.quitting = true;
    }
}
