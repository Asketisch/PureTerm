use pureterm_core::config::AppConfig;
use pureterm_core::hotkey::HotkeyService;

use crate::tabs::TabManager;

/// Main application state.
pub struct App {
    pub config: AppConfig,
    pub tab_manager: TabManager,
    pub hotkey_service: HotkeyService,
    pub show_settings: bool,
    pub search_visible: bool,
    pub search_query: String,
    pub quitting: bool,
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
        }
    }

    /// Process a key event and return if consumed.
    pub fn handle_key(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool) -> bool {
        let modifiers = pureterm_core::hotkey::Modifiers {
            ctrl,
            alt,
            shift,
            super_key: false,
        };

        if self.hotkey_service.matches("tab_new", key, modifiers) {
            // Handled by the main loop
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
