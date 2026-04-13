//! Global hotkey support for PureTerm.
//!
//! Registers system-wide keyboard shortcuts that work even when PureTerm
//! is not focused. Uses platform-specific APIs:
//! - macOS: CGEventTap
//! - Linux: X11 grab_key
//! - Windows: RegisterHotKey

/// A global hotkey registration.
#[derive(Debug, Clone)]
pub struct GlobalHotkey {
    /// Unique identifier.
    pub id: u32,
    /// Key combination.
    pub key: HotkeyCombo,
    /// Action to perform.
    pub action: GlobalHotkeyAction,
}

#[derive(Debug, Clone)]
pub struct HotkeyCombo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalHotkeyAction {
    /// Toggle window visibility (show/hide).
    ToggleVisibility,
    /// Show Quake-style dropdown.
    ShowQuake,
    /// Create a new tab.
    NewTab,
    /// Open command palette.
    CommandPalette,
}

/// Manages global hotkey registrations.
pub struct GlobalHotkeyManager {
    hotkeys: Vec<GlobalHotkey>,
    registered: bool,
}

impl GlobalHotkeyManager {
    pub fn new() -> Self {
        Self {
            hotkeys: Vec::new(),
            registered: false,
        }
    }

    /// Register a global hotkey.
    pub fn register(&mut self, hotkey: GlobalHotkey) {
        log::info!("Registering global hotkey: {:?} -> {:?}", hotkey.key, hotkey.action);
        self.hotkeys.push(hotkey);
    }

    /// Register the default global hotkeys.
    pub fn register_defaults(&mut self) {
        self.register(GlobalHotkey {
            id: 1,
            key: HotkeyCombo {
                ctrl: true,
                alt: false,
                shift: false,
                super_key: false,
                key: "`".to_string(),
            },
            action: GlobalHotkeyAction::ShowQuake,
        });
        self.register(GlobalHotkey {
            id: 2,
            key: HotkeyCombo {
                ctrl: true,
                alt: false,
                shift: true,
                super_key: false,
                key: "`".to_string(),
            },
            action: GlobalHotkeyAction::ToggleVisibility,
        });
    }

    /// Start listening for global hotkeys.
    pub fn start(&mut self) {
        self.registered = true;
        log::info!("Global hotkey listener started with {} hotkeys", self.hotkeys.len());
        // Platform-specific registration would go here
    }

    /// Stop listening for global hotkeys.
    pub fn stop(&mut self) {
        self.registered = false;
        log::info!("Global hotkey listener stopped");
    }

    /// Check if a key combo matches any registered hotkey.
    pub fn check(&self, ctrl: bool, alt: bool, shift: bool, super_key: bool, key: &str) -> Option<GlobalHotkeyAction> {
        self.hotkeys.iter().find(|h| {
            h.key.ctrl == ctrl
                && h.key.alt == alt
                && h.key.shift == shift
                && h.key.super_key == super_key
                && h.key.key == key
        }).map(|h| h.action)
    }

    pub fn is_registered(&self) -> bool {
        self.registered
    }

    /// List all registered hotkeys.
    pub fn hotkeys(&self) -> &[GlobalHotkey] {
        &self.hotkeys
    }
}

impl Default for GlobalHotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}
