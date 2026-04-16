//! Key binding system.
//!
//! Based on Ghostty's Binding.zig (~5000 lines, 165KB).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hash;

use super::{Key, KeyEvent, Mods};

/// Trigger key - what causes a binding to fire.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggerKey {
    /// Physical key press.
    Physical(Key),
    /// Unicode character input.
    Unicode(char),
    /// Catch-all for any key.
    CatchAll,
}

/// Trigger condition for a binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Trigger {
    /// The key that triggers this binding.
    pub key: TriggerKey,
    /// Required modifiers (subset matching).
    pub mods: Mods,
}

impl Trigger {
    /// Create a trigger for a physical key.
    pub fn physical(key: Key) -> Self {
        Self {
            key: TriggerKey::Physical(key),
            mods: Mods::empty(),
        }
    }

    /// Create a trigger for a physical key with modifiers.
    pub fn physical_with_mods(key: Key, mods: u16) -> Self {
        Self {
            key: TriggerKey::Physical(key),
            mods: Mods::from_bits(mods),
        }
    }

    /// Create a trigger for a Unicode character.
    pub fn unicode(c: char) -> Self {
        Self {
            key: TriggerKey::Unicode(c),
            mods: Mods::empty(),
        }
    }

    /// Create a catch-all trigger.
    pub fn catch_all() -> Self {
        Self {
            key: TriggerKey::CatchAll,
            mods: Mods::empty(),
        }
    }

    /// Check if this trigger matches a key event.
    pub fn matches(&self, event: &KeyEvent) -> bool {
        match &self.key {
            TriggerKey::Physical(key) => {
                if event.key != *key {
                    return false;
                }
                // Check modifiers (must have at least the required mods)
                let effective = event.effective_mods();
                effective.has_all(self.mods)
            }
            TriggerKey::Unicode(c) => {
                if let Some(event_c) = event.utf8.chars().next() {
                    if event_c != *c {
                        return false;
                    }
                    event.effective_mods().has_all(self.mods)
                } else {
                    false
                }
            }
            TriggerKey::CatchAll => {
                // Catch-all matches anything with the required modifiers
                event.effective_mods().has_all(self.mods)
            }
        }
    }
}

/// Binding flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingFlags(u8);

impl BindingFlags {
    pub const NONE: u8 = 0;
    pub const APPINIT: u8 = 0x01; // Available at app init
    pub const GLOBAL: u8 = 0x02; // Works in all contexts
    pub const NO_REPEAT: u8 = 0x04; // Don't repeat this key
    pub const LEADER: u8 = 0x08; // This is a leader key sequence
    pub const BREAK: u8 = 0x10; // Break from binding sequence

    pub fn new() -> Self {
        Self(Self::NONE)
    }

    pub fn contains(&self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    pub fn insert(&mut self, flag: u8) {
        self.0 |= flag;
    }

    pub fn is_empty(&self) -> bool {
        self.0 == Self::NONE
    }
}

/// Binding action - what happens when a binding fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BindingAction {
    // === Terminal actions ===
    /// Copy selection to clipboard.
    Copy,
    /// Paste from clipboard.
    Paste,
    /// Paste from selection.
    PasteSelection,
    /// Reset terminal.
    Reset,
    /// Enter search mode.
    Search,
    /// Scroll to top of scrollback.
    ScrollToTop,
    /// Scroll to bottom of scrollback.
    ScrollToBottom,
    /// Paste from bracketed paste.
    PasteBracketed,
    /// Copy to clipboard as HTML.
    CopyHtml,

    // === Font actions ===
    /// Increase font size.
    FontSizeIncrease,
    /// Decrease font size.
    FontSizeDecrease,
    /// Reset font size.
    FontSizeReset,
    /// Increase font size (alternate).
    FontSizeUp,
    /// Decrease font size (alternate).
    FontSizeDown,

    // === Window actions ===
    /// Create new window.
    NewWindow,
    /// Create new tab.
    NewTab,
    /// Create new split.
    NewSplit,
    /// Close current pane.
    Close,
    /// Toggle fullscreen.
    ToggleFullscreen,
    /// Toggle maximized.
    ToggleMaximized,
    /// Toggle always on top.
    ToggleAlwaysOnTop,
    /// Minimize window.
    Minimize,
    /// Zoom current pane.
    Zoom,

    // === Config actions ===
    /// Open config file.
    OpenConfig,
    /// Reload config.
    ReloadConfig,
    /// Switch to next theme.
    NextTheme,
    /// Switch to previous theme.
    PreviousTheme,

    // === Quick terminal ===
    /// Toggle quick terminal.
    ToggleQuickTerminal,
    /// Show quick terminal.
    ShowQuickTerminal,
    /// Hide quick terminal.
    HideQuickTerminal,

    // === Inspector ===
    /// Toggle inspector.
    ToggleInspector,

    // === Special ===
    /// Clear scrollback.
    ClearScrollback,
    /// Clear selection.
    ClearSelection,
    /// Insert newline.
    NewLine,
    /// Scroll one line up.
    ScrollLineUp,
    /// Scroll one line down.
    ScrollLineDown,
    /// Scroll one page up.
    ScrollPageUp,
    /// Scroll one page down.
    ScrollPageDown,
    /// Select all.
    SelectAll,
    /// Undo selection.
    UndoSelection,

    // === Mouse ===
    /// Extend selection to current position.
    ExtendSelection,
    /// Open selected URL.
    OpenSelection,

    // === Unicode ===
    /// Insert unicode character.
    Unicode,

    // === Pass through ===
    /// Pass key to terminal.
    PassThrough,

    // === No operation ===
    /// Do nothing.
    NoOp,

    // === Unknown ===
    /// Unknown action (for parsing errors).
    Unknown(String),
}

impl BindingAction {
    /// Parse an action name string.
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "copy" => BindingAction::Copy,
            "paste" => BindingAction::Paste,
            "paste_selection" | "pasteselection" => BindingAction::PasteSelection,
            "reset" => BindingAction::Reset,
            "search" => BindingAction::Search,
            "scroll_to_top" | "scrolltotop" => BindingAction::ScrollToTop,
            "scroll_to_bottom" | "scrolltobottom" => BindingAction::ScrollToBottom,
            "paste_bracketed" | "pastebracketed" => BindingAction::PasteBracketed,
            "copy_html" | "copyhtml" => BindingAction::CopyHtml,
            "font_size_increase" | "fontsizeincrease" => BindingAction::FontSizeIncrease,
            "font_size_decrease" | "fontsizedecrease" => BindingAction::FontSizeDecrease,
            "font_size_reset" | "fontsizereset" => BindingAction::FontSizeReset,
            "font_size_up" | "fontsizeup" => BindingAction::FontSizeUp,
            "font_size_down" | "fontsizedown" => BindingAction::FontSizeDown,
            "new_window" | "newwindow" => BindingAction::NewWindow,
            "new_tab" | "newtab" => BindingAction::NewTab,
            "new_split" | "newsplit" => BindingAction::NewSplit,
            "close" => BindingAction::Close,
            "toggle_fullscreen" | "togglefullscreen" => BindingAction::ToggleFullscreen,
            "toggle_maximized" | "togglemaximized" => BindingAction::ToggleMaximized,
            "toggle_always_on_top" | "togglealwaysontop" => BindingAction::ToggleAlwaysOnTop,
            "minimize" => BindingAction::Minimize,
            "zoom" => BindingAction::Zoom,
            "open_config" | "openconfig" => BindingAction::OpenConfig,
            "reload_config" | "reloadconfig" => BindingAction::ReloadConfig,
            "next_theme" | "nexttheme" => BindingAction::NextTheme,
            "previous_theme" | "previoustheme" => BindingAction::PreviousTheme,
            "toggle_quick_terminal" | "togglequickterminal" => BindingAction::ToggleQuickTerminal,
            "show_quick_terminal" | "showquickterminal" => BindingAction::ShowQuickTerminal,
            "hide_quick_terminal" | "hidequickterminal" => BindingAction::HideQuickTerminal,
            "toggle_inspector" | "toggleinspector" => BindingAction::ToggleInspector,
            "clear_scrollback" | "clearscrollback" => BindingAction::ClearScrollback,
            "clear_selection" | "clearselection" => BindingAction::ClearSelection,
            "newline" | "new_line" => BindingAction::NewLine,
            "scroll_line_up" | "scrolllineup" => BindingAction::ScrollLineUp,
            "scroll_line_down" | "scrolllinedown" => BindingAction::ScrollLineDown,
            "scroll_page_up" | "scrollpageup" => BindingAction::ScrollPageUp,
            "scroll_page_down" | "scrollpagedown" => BindingAction::ScrollPageDown,
            "select_all" | "selectall" => BindingAction::SelectAll,
            "undo_selection" | "undoselection" => BindingAction::UndoSelection,
            "extend_selection" | "extendselection" => BindingAction::ExtendSelection,
            "open_selection" | "openselection" => BindingAction::OpenSelection,
            "unicode" => BindingAction::Unicode,
            "passthrough" | "pass_through" => BindingAction::PassThrough,
            "noop" | "no_op" => BindingAction::NoOp,
            _ => BindingAction::Unknown(s.to_string()),
        }
    }
}

/// Convert a BindingAction to a string key for HashMap storage.
fn action_to_key(action: &BindingAction) -> String {
    match action {
        BindingAction::Copy => "copy".to_string(),
        BindingAction::Paste => "paste".to_string(),
        BindingAction::PasteSelection => "paste_selection".to_string(),
        BindingAction::Reset => "reset".to_string(),
        BindingAction::Search => "search".to_string(),
        BindingAction::ScrollToTop => "scroll_to_top".to_string(),
        BindingAction::ScrollToBottom => "scroll_to_bottom".to_string(),
        BindingAction::PasteBracketed => "paste_bracketed".to_string(),
        BindingAction::CopyHtml => "copy_html".to_string(),
        BindingAction::FontSizeIncrease => "font_size_increase".to_string(),
        BindingAction::FontSizeDecrease => "font_size_decrease".to_string(),
        BindingAction::FontSizeReset => "font_size_reset".to_string(),
        BindingAction::FontSizeUp => "font_size_up".to_string(),
        BindingAction::FontSizeDown => "font_size_down".to_string(),
        BindingAction::NewWindow => "new_window".to_string(),
        BindingAction::NewTab => "new_tab".to_string(),
        BindingAction::NewSplit => "new_split".to_string(),
        BindingAction::Close => "close".to_string(),
        BindingAction::ToggleFullscreen => "toggle_fullscreen".to_string(),
        BindingAction::ToggleMaximized => "toggle_maximized".to_string(),
        BindingAction::ToggleAlwaysOnTop => "toggle_always_on_top".to_string(),
        BindingAction::Minimize => "minimize".to_string(),
        BindingAction::Zoom => "zoom".to_string(),
        BindingAction::OpenConfig => "open_config".to_string(),
        BindingAction::ReloadConfig => "reload_config".to_string(),
        BindingAction::NextTheme => "next_theme".to_string(),
        BindingAction::PreviousTheme => "previous_theme".to_string(),
        BindingAction::ToggleQuickTerminal => "toggle_quick_terminal".to_string(),
        BindingAction::ShowQuickTerminal => "show_quick_terminal".to_string(),
        BindingAction::HideQuickTerminal => "hide_quick_terminal".to_string(),
        BindingAction::ToggleInspector => "toggle_inspector".to_string(),
        BindingAction::ClearScrollback => "clear_scrollback".to_string(),
        BindingAction::ClearSelection => "clear_selection".to_string(),
        BindingAction::NewLine => "newline".to_string(),
        BindingAction::ScrollLineUp => "scroll_line_up".to_string(),
        BindingAction::ScrollLineDown => "scroll_line_down".to_string(),
        BindingAction::ScrollPageUp => "scroll_page_up".to_string(),
        BindingAction::ScrollPageDown => "scroll_page_down".to_string(),
        BindingAction::SelectAll => "select_all".to_string(),
        BindingAction::UndoSelection => "undo_selection".to_string(),
        BindingAction::ExtendSelection => "extend_selection".to_string(),
        BindingAction::OpenSelection => "open_selection".to_string(),
        BindingAction::Unicode => "unicode".to_string(),
        BindingAction::PassThrough => "passthrough".to_string(),
        BindingAction::NoOp => "noop".to_string(),
        BindingAction::Unknown(s) => s.clone(),
    }
}

/// A single key binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    /// The trigger condition.
    pub trigger: Trigger,
    /// The action to perform.
    pub action: BindingAction,
    /// Binding flags.
    pub flags: BindingFlags,
    /// Optional argument for the action (e.g., unicode character).
    pub arg: Option<String>,
}

impl Binding {
    /// Create a new binding.
    pub fn new(trigger: Trigger, action: BindingAction) -> Self {
        Self {
            trigger,
            action,
            flags: BindingFlags::new(),
            arg: None,
        }
    }

    /// Create with flags.
    pub fn with_flags(mut self, flags: BindingFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Create with an argument.
    pub fn with_arg(mut self, arg: String) -> Self {
        self.arg = Some(arg);
        self
    }
}

/// A set of key bindings with multi-key sequence support.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BindingSet {
    /// Primary bindings: trigger -> binding.
    bindings: HashMap<Trigger, Binding>,
    /// Reverse mapping: action name -> triggers (for showing bindings for an action).
    reverse: HashMap<String, Vec<Trigger>>,
    /// Multi-key sequences: first key -> nested bindings.
    sequences: HashMap<Trigger, HashMap<Trigger, Binding>>,
    /// Currently active leader key sequence.
    active_sequence: Option<(Trigger, HashMap<Trigger, Binding>)>,
    /// Sequence timeout in milliseconds.
    sequence_timeout_ms: u32,
}

impl BindingSet {
    /// Create a new empty binding set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a binding.
    pub fn add(&mut self, binding: Binding) {
        let trigger = binding.trigger.clone();
        let action_key = action_to_key(&binding.action);

        // Add to primary bindings
        self.bindings.insert(trigger.clone(), binding);

        // Add to reverse mapping
        self.reverse.entry(action_key).or_insert_with(Vec::new).push(trigger);
    }

    /// Remove a binding by trigger.
    pub fn remove(&mut self, trigger: &Trigger) -> Option<Binding> {
        if let Some(binding) = self.bindings.remove(trigger) {
            // Remove from reverse mapping
            let action_key = action_to_key(&binding.action);
            if let Some(triggers) = self.reverse.get_mut(&action_key) {
                triggers.retain(|t| t != trigger);
            }
            Some(binding)
        } else {
            None
        }
    }

    /// Find a matching binding for a key event.
    pub fn find(&mut self, event: &KeyEvent) -> Option<Binding> {
        // Iterate over all bindings to find a match
        // The trigger's mods in the HashMap key don't need to match exactly
        // because we check trigger.matches(event) which handles modifier logic
        for binding in self.bindings.values() {
            if binding.trigger.matches(event) {
                return Some(binding.clone());
            }
        }

        // Check multi-key sequences
        let result = {
            let active_seq = self.active_sequence.take();
            if let Some((_leader, nested)) = active_seq {
                let result = nested.get(&Trigger::physical(event.key)).cloned();
                if result.is_none() {
                    // Restore if no match
                    self.active_sequence = Some((_leader, nested));
                }
                result
            } else {
                None
            }
        };

        if let Some(binding) = result {
            if binding.trigger.matches(event) {
                return Some(binding);
            }
            // If match failed, restore
            self.active_sequence = None;
        }

        // Check if this key starts a sequence
        if self.sequences.contains_key(&Trigger::physical(event.key)) {
            let trigger = Trigger::physical(event.key);
            if trigger.matches(event) {
                // Start the sequence
                if let Some(nested) = self.sequences.get(&trigger) {
                    self.active_sequence = Some((trigger, nested.clone()));
                }
                return None; // Waiting for next key
            }
        }

        None
    }

    /// Add a multi-key sequence binding.
    pub fn add_sequence(&mut self, first: Trigger, second: Trigger, binding: Binding) {
        self.sequences
            .entry(first)
            .or_insert_with(HashMap::new)
            .insert(second, binding);
    }

    /// Clear the active sequence.
    pub fn clear_sequence(&mut self) {
        self.active_sequence = None;
    }

    /// Check if there's an active sequence.
    pub fn has_active_sequence(&self) -> bool {
        self.active_sequence.is_some()
    }

    /// Get the current active sequence trigger.
    pub fn active_sequence_trigger(&self) -> Option<&Trigger> {
        self.active_sequence.as_ref().map(|(t, _)| t)
    }

    /// Get all bindings for an action.
    pub fn bindings_for(&self, action: BindingAction) -> Option<&Vec<Trigger>> {
        let key = action_to_key(&action);
        self.reverse.get(&key)
    }

    /// Get all triggers for an action.
    pub fn triggers_for(&self, action: BindingAction) -> Vec<Trigger> {
        let key = action_to_key(&action);
        self.reverse.get(&key).cloned().unwrap_or_default()
    }

    /// Iterate over all bindings.
    pub fn iter(&self) -> impl Iterator<Item = &Binding> {
        self.bindings.values()
    }

    /// Get the number of bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Set sequence timeout.
    pub fn set_sequence_timeout(&mut self, ms: u32) {
        self.sequence_timeout_ms = ms;
    }

    /// Get sequence timeout.
    pub fn sequence_timeout(&self) -> u32 {
        self.sequence_timeout_ms
    }
}

/// Default key bindings for the terminal.
pub fn default_bindings() -> BindingSet {
    let mut bindings = BindingSet::new();

    // Copy/Paste
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::C, Mods::CTRL | Mods::SHIFT),
        BindingAction::Copy,
    ));
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::V, Mods::CTRL | Mods::SHIFT),
        BindingAction::Paste,
    ));
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::C, Mods::CTRL),
        BindingAction::Copy,
    ));
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::V, Mods::CTRL),
        BindingAction::Paste,
    ));

    // Font size
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::Equals, Mods::CTRL | Mods::SHIFT),
        BindingAction::FontSizeIncrease,
    ));
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::Minus, Mods::CTRL | Mods::SHIFT),
        BindingAction::FontSizeDecrease,
    ));
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::Digit0, Mods::CTRL | Mods::SHIFT),
        BindingAction::FontSizeReset,
    ));

    // Scroll
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::PageUp, 0),
        BindingAction::ScrollPageUp,
    ));
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::PageDown, 0),
        BindingAction::ScrollPageDown,
    ));
    bindings.add(Binding::new(
        Trigger::physical(Key::Home),
        BindingAction::ScrollToTop,
    ));
    bindings.add(Binding::new(
        Trigger::physical(Key::End),
        BindingAction::ScrollToBottom,
    ));

    // Search
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::F, Mods::CTRL | Mods::SHIFT),
        BindingAction::Search,
    ));

    // Reset
    bindings.add(Binding::new(
        Trigger::physical_with_mods(Key::K, Mods::CTRL | Mods::SHIFT),
        BindingAction::Reset,
    ));

    bindings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::KeyAction;

    #[test]
    fn test_trigger_physical() {
        let trigger = Trigger::physical(Key::C);
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::C,
            Mods::from_bits(Mods::CTRL),
            "c".to_string(),
        );
        assert!(trigger.matches(&event));
    }

    #[test]
    fn test_trigger_with_mods() {
        let trigger = Trigger::physical_with_mods(Key::C, Mods::CTRL);
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::C,
            Mods::from_bits(Mods::CTRL | Mods::SHIFT),
            "c".to_string(),
        );
        assert!(trigger.matches(&event));

        let event_no_ctrl = KeyEvent::new(
            KeyAction::Press,
            Key::C,
            Mods::from_bits(Mods::SHIFT),
            "c".to_string(),
        );
        assert!(!trigger.matches(&event_no_ctrl));
    }

    #[test]
    fn test_binding_action_parse() {
        assert_eq!(BindingAction::parse("copy"), BindingAction::Copy);
        assert_eq!(BindingAction::parse("Paste"), BindingAction::Paste);
        assert_eq!(BindingAction::parse("unknown_action"), BindingAction::Unknown("unknown_action".to_string()));
    }

    #[test]
    fn test_binding_set() {
        let mut bindings = BindingSet::new();

        bindings.add(Binding::new(
            Trigger::physical_with_mods(Key::C, Mods::CTRL),
            BindingAction::Copy,
        ));

        assert_eq!(bindings.len(), 1);
        assert!(!bindings.is_empty());

        let removed = bindings.remove(&Trigger::physical_with_mods(Key::C, Mods::CTRL));
        assert!(removed.is_some());
        assert!(bindings.is_empty());
    }

    #[test]
    fn test_find_binding() {
        let mut bindings = default_bindings();

        let event = KeyEvent::new(
            KeyAction::Press,
            Key::C,
            Mods::from_bits(Mods::CTRL),
            "c".to_string(),
        );

        let found = bindings.find(&event);
        assert!(found.is_some());
        assert_eq!(found.unwrap().action, BindingAction::Copy);
    }

    #[test]
    fn test_catch_all() {
        let mut bindings = BindingSet::new();

        bindings.add(Binding::new(
            Trigger::catch_all(),
            BindingAction::NoOp,
        ));

        let event = KeyEvent::new(
            KeyAction::Press,
            Key::Z,
            Mods::empty(),
            "z".to_string(),
        );

        let found = bindings.find(&event);
        assert!(found.is_some());
        assert_eq!(found.unwrap().action, BindingAction::NoOp);
    }

    #[test]
    fn test_default_bindings() {
        let bindings = default_bindings();
        assert!(!bindings.is_empty());
    }

    #[test]
    fn test_triggers_for() {
        let bindings = default_bindings();
        let triggers = bindings.triggers_for(BindingAction::Copy);
        assert!(!triggers.is_empty());
    }
}
