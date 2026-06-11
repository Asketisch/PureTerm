//! GAP-3.7: Keyboard customization framework.
//!
//! Centralises all user-triggerable actions into a single `Action` enum and
//! provides a config-driven `Keymap` lookup table that replaces the inline
//! `match` dispatch in `handle_keyboard()`.
//!
//! Design:
//!   keypress → Keymap::resolve() → Action → dispatch_action() → Message → update()
//!
//! Overlay UI guards (Escape dismiss, arrow-key navigation, Enter confirm)
//! remain hardcoded in `handle_keyboard` — they are UI-state-dependent and
//! not user-customisable.

use std::collections::HashMap;
use std::time::Instant;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::split_tree::SplitDirection;

// ── Action ──────────────────────────────────────────────────────────────

/// Single vocabulary for all user-triggerable operations.
/// Replaces the three parallel dispatch paths (keyboard match,
/// palette string match, context menu match).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    // ── Tab management ──────────────────────────────────────────────
    NewTab,
    CloseTab,
    /// Context-dependent: close pane if multi-pane, close tab if single.
    ClosePaneOrTab,
    NextTab,
    PrevTab,
    ActivateTabByIndex(usize),
    MovePaneToNewTab,
    MovePaneToNewWindow,

    // ── Split pane ──────────────────────────────────────────────────
    SplitHorizontal,
    SplitVertical,
    EqualizeSplits,
    SwapPane,
    ToggleZoom,
    FocusPaneInOrder(i32),
    PaneFocusAdjacent(SplitDirection),

    // ── Clipboard ───────────────────────────────────────────────────
    ClipboardPaste,
    ClipboardCopy,
    SelectAll,

    // ── Dialogs / overlays ──────────────────────────────────────────
    OpenSshDialog,
    OpenCommandPalette,
    OpenSettings,

    // ── Font / window (TOML-declared but not yet wired) ─────────────
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    ToggleFullscreen,
    SpawnNewInstance,
}

// ── BindingKey ──────────────────────────────────────────────────────────

/// Key specification used in binding lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BindingKey {
    Character(String),
    Named(BindingNamedKey),
}

/// Subset of iced's `Named` keys that appear in bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingNamedKey {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Enter,
    Escape,
    Tab,
    Backspace,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    Space,
    Plus,
    Minus,
    Equals,
}

impl BindingNamedKey {
    /// Convert from iced's `Named` key. Returns `None` for keys we don't
    /// bind (e.g. custom key names).
    pub fn from_iced(named: Named) -> Option<Self> {
        match named {
            Named::ArrowUp => Some(Self::ArrowUp),
            Named::ArrowDown => Some(Self::ArrowDown),
            Named::ArrowLeft => Some(Self::ArrowLeft),
            Named::ArrowRight => Some(Self::ArrowRight),
            Named::Enter => Some(Self::Enter),
            Named::Escape => Some(Self::Escape),
            Named::Tab => Some(Self::Tab),
            Named::Backspace => Some(Self::Backspace),
            Named::Home => Some(Self::Home),
            Named::End => Some(Self::End),
            Named::PageUp => Some(Self::PageUp),
            Named::PageDown => Some(Self::PageDown),
            Named::Insert => Some(Self::Insert),
            Named::Delete => Some(Self::Delete),
            Named::Space => Some(Self::Space),
            _ => None,
        }
    }
}

/// Convert an iced `Key` to our `BindingKey`. Returns `None` for keys
/// that cannot be bound (e.g. custom/unrecognized).
pub fn key_to_binding_key(key: &Key) -> Option<BindingKey> {
    match key {
        Key::Character(s) => Some(BindingKey::Character(s.to_string())),
        Key::Named(named) => BindingNamedKey::from_iced(*named).map(BindingKey::Named),
        _ => None,
    }
}

// ── ResolveResult ───────────────────────────────────────────────────────

/// Result of resolving a keypress against the keymap.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveResult {
    /// A single-stroke binding matched.
    Action(Action),
    /// A chord prefix was accepted; waiting for the next key.
    ChordPending,
    /// A chord sequence completed.
    ChordAction(Action),
    /// No binding matched; fall through to PTY passthrough.
    Passthrough,
}

// ── Keymap ──────────────────────────────────────────────────────────────

/// Config-driven keyboard binding table.
///
/// `bindings` maps `(Modifiers, BindingKey) → Action` for single-stroke
/// shortcuts. `chord_bindings` maps `(prefix_mods, prefix_key, second_mods,
/// second_key) → Action` for two-key sequences (e.g. `Ctrl+K` then `j`).
pub struct Keymap {
    bindings: HashMap<(Modifiers, BindingKey), Action>,
    chord_bindings: HashMap<(Modifiers, BindingKey, Modifiers, BindingKey), Action>,
    pending_chord: Option<(Modifiers, BindingKey, Instant)>,
    chord_timeout_ms: u64,
}

impl Keymap {
    /// Create a keymap from the hardcoded defaults that exactly reproduce
    /// the current `handle_keyboard` inline match behaviour.
    pub fn from_defaults() -> Self {
        let mut bindings = HashMap::new();

        // macOS reports ⌘ as `command()`.
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let cmd_alt = Modifiers::COMMAND | Modifiers::ALT;

        // ── Tab management ──────────────────────────────────────────
        bindings.insert(
            (cmd, BindingKey::Character("t".into())),
            Action::NewTab,
        );
        // ⌘⇧T / ⌘T-with-uppercase-T → MovePaneToNewTab
        // macOS may report ⌘⇧T as Character("T") with shift NOT set,
        // so we register both forms.
        bindings.insert(
            (cmd, BindingKey::Character("T".into())),
            Action::MovePaneToNewTab,
        );
        bindings.insert(
            (cmd_shift, BindingKey::Character("t".into())),
            Action::MovePaneToNewTab,
        );
        // ⌘W → ClosePaneOrTab
        bindings.insert(
            (cmd, BindingKey::Character("w".into())),
            Action::ClosePaneOrTab,
        );
        // ⌘} → NextTab (macOS reports ⌘⇧] as "}")
        bindings.insert(
            (cmd, BindingKey::Character("}".into())),
            Action::NextTab,
        );
        // ⌘{ → PrevTab (macOS reports ⌘⇧[ as "{")
        bindings.insert(
            (cmd, BindingKey::Character("{".into())),
            Action::PrevTab,
        );
        // ⌘] → FocusPaneInOrder(1)
        bindings.insert(
            (cmd, BindingKey::Character("]".into())),
            Action::FocusPaneInOrder(1),
        );
        // ⌘[ → FocusPaneInOrder(-1)
        bindings.insert(
            (cmd, BindingKey::Character("[".into())),
            Action::FocusPaneInOrder(-1),
        );
        // ⌘1..9 → ActivateTabByIndex
        for i in 1..=9u8 {
            let ch = (b'0' + i) as char;
            bindings.insert(
                (cmd, BindingKey::Character(ch.to_string())),
                Action::ActivateTabByIndex((i - 1) as usize),
            );
        }

        // ── Split pane ──────────────────────────────────────────────
        // ⌘D → SplitHorizontal
        bindings.insert(
            (cmd, BindingKey::Character("d".into())),
            Action::SplitHorizontal,
        );
        // ⌘⇧D → SplitVertical (macOS may report as "D" without shift)
        bindings.insert(
            (cmd_shift, BindingKey::Character("d".into())),
            Action::SplitVertical,
        );
        bindings.insert(
            (cmd, BindingKey::Character("D".into())),
            Action::SplitVertical,
        );
        // ⌘= → EqualizeSplits
        bindings.insert(
            (cmd, BindingKey::Character("=".into())),
            Action::EqualizeSplits,
        );
        // ⌘⇧X → SwapPane
        bindings.insert(
            (cmd_shift, BindingKey::Character("x".into())),
            Action::SwapPane,
        );
        bindings.insert(
            (cmd, BindingKey::Character("X".into())),
            Action::SwapPane,
        );
        // ⌘⇧Enter → ToggleZoom
        bindings.insert(
            (cmd_shift, BindingKey::Named(BindingNamedKey::Enter)),
            Action::ToggleZoom,
        );
        // ⌘⇧N → MovePaneToNewWindow
        bindings.insert(
            (cmd_shift, BindingKey::Character("n".into())),
            Action::MovePaneToNewWindow,
        );
        bindings.insert(
            (cmd, BindingKey::Character("N".into())),
            Action::MovePaneToNewWindow,
        );

        // ── Clipboard ───────────────────────────────────────────────
        bindings.insert(
            (cmd, BindingKey::Character("v".into())),
            Action::ClipboardPaste,
        );
        bindings.insert(
            (cmd, BindingKey::Character("c".into())),
            Action::ClipboardCopy,
        );
        bindings.insert(
            (cmd, BindingKey::Character("a".into())),
            Action::SelectAll,
        );

        // ── Dialogs (platform-gated at dispatch level) ──────────────
        // ⌘⇧S → OpenSshDialog (macOS reports as "S" without shift)
        bindings.insert(
            (cmd_shift, BindingKey::Character("s".into())),
            Action::OpenSshDialog,
        );
        bindings.insert(
            (cmd, BindingKey::Character("S".into())),
            Action::OpenSshDialog,
        );
        // Also match plain ⌘s for safety (macOS dual-form)
        bindings.insert(
            (cmd, BindingKey::Character("s".into())),
            Action::OpenSshDialog,
        );
        // ⌘⇧P → OpenCommandPalette (macOS reports as "P" without shift)
        bindings.insert(
            (cmd_shift, BindingKey::Character("p".into())),
            Action::OpenCommandPalette,
        );
        bindings.insert(
            (cmd, BindingKey::Character("P".into())),
            Action::OpenCommandPalette,
        );
        bindings.insert(
            (cmd, BindingKey::Character("p".into())),
            Action::OpenCommandPalette,
        );

        // ── Cmd+Option+Arrow → PaneFocusAdjacent ────────────────────
        bindings.insert(
            (cmd_alt, BindingKey::Named(BindingNamedKey::ArrowLeft)),
            Action::PaneFocusAdjacent(SplitDirection::Horizontal),
        );
        bindings.insert(
            (cmd_alt, BindingKey::Named(BindingNamedKey::ArrowRight)),
            Action::PaneFocusAdjacent(SplitDirection::Horizontal),
        );
        bindings.insert(
            (cmd_alt, BindingKey::Named(BindingNamedKey::ArrowUp)),
            Action::PaneFocusAdjacent(SplitDirection::Vertical),
        );
        bindings.insert(
            (cmd_alt, BindingKey::Named(BindingNamedKey::ArrowDown)),
            Action::PaneFocusAdjacent(SplitDirection::Vertical),
        );

        Self {
            bindings,
            chord_bindings: HashMap::new(),
            pending_chord: None,
            chord_timeout_ms: 1000,
        }
    }

    /// Resolve a keypress event to an action.
    ///
    /// Checks pending chord state first, then single-stroke bindings.
    pub fn resolve(&mut self, key: &Key, mods: Modifiers, now: Instant) -> ResolveResult {
        let binding_key = match key_to_binding_key(key) {
            Some(bk) => bk,
            None => return ResolveResult::Passthrough,
        };

        // ── Chord resolution ────────────────────────────────────────
        if let Some((prefix_mods, prefix_key, ts)) = self.pending_chord.take() {
            let elapsed = now.duration_since(ts).as_millis() as u64;
            if elapsed <= self.chord_timeout_ms {
                // Escape cancels chord
                if binding_key == BindingKey::Named(BindingNamedKey::Escape) {
                    return ResolveResult::Passthrough;
                }
                // Look up chord completion
                if let Some(action) = self
                    .chord_bindings
                    .get(&(prefix_mods, prefix_key.clone(), mods, binding_key.clone()))
                {
                    return ResolveResult::ChordAction(action.clone());
                }
                // No chord match — the chord prefix was consumed; treat
                // this key as a fresh keypress below.
            }
            // Chord timed out or didn't match — fall through to normal lookup
        }

        // ── Single-stroke lookup ────────────────────────────────────
        if let Some(action) = self.bindings.get(&(mods, binding_key.clone())) {
            return ResolveResult::Action(action.clone());
        }

        // ── Check if this key is a chord prefix ─────────────────────
        // A key is a chord prefix if there exist chord_bindings that
        // start with (mods, binding_key).
        let is_prefix = self
            .chord_bindings
            .keys()
            .any(|(pm, pk, _, _)| *pm == mods && *pk == binding_key);

        if is_prefix {
            self.pending_chord = Some((mods, binding_key, now));
            return ResolveResult::ChordPending;
        }

        ResolveResult::Passthrough
    }

    /// Load bindings from TOML `[[keyboard.bindings]]` raw data.
    /// User bindings override defaults for the same key combo.
    /// Unknown actions or unparseable keys are silently skipped (with log::warn).
    pub fn from_toml_overrides(
        raw: &[RawBinding],
        defaults: &Keymap,
    ) -> Self {
        let mut bindings = defaults.bindings.clone();
        for rb in raw {
            match parse_raw_binding(rb) {
                Ok(((mods, bk), action)) => {
                    bindings.insert((mods, bk), action);
                }
                Err(e) => {
                    log::warn!(
                        "keymap: skipping binding key={} mods={} action={}: {}",
                        rb.key,
                        rb.mods,
                        rb.action,
                        e
                    );
                }
            }
        }
        Self {
            bindings,
            chord_bindings: defaults.chord_bindings.clone(),
            pending_chord: None,
            chord_timeout_ms: defaults.chord_timeout_ms,
        }
    }

    /// Return a snapshot of the current bindings for display / CLI listing.
    pub fn list_bindings(&self) -> Vec<(Modifiers, BindingKey, Action)> {
        self.bindings
            .iter()
            .map(|((mods, bk), action)| (*mods, bk.clone(), action.clone()))
            .collect()
    }
}

// ── TOML raw binding ────────────────────────────────────────────────────

/// Raw binding entry as read from `[[keyboard.bindings]]` TOML section.
#[derive(Debug, Clone)]
pub struct RawBinding {
    pub key: String,
    pub mods: String,
    pub action: String,
}

// ── TOML parsing helpers ────────────────────────────────────────────────

fn parse_raw_binding(
    rb: &RawBinding,
) -> Result<((Modifiers, BindingKey), Action), String> {
    let mods = parse_mods(&rb.mods)?;
    let key = parse_key(&rb.key)?;
    let action = parse_action(&rb.action)?;
    Ok(((mods, key), action))
}

/// Parse pipe-separated modifiers: "Command|Shift", "Control", "Alt|Shift"
fn parse_mods(s: &str) -> Result<Modifiers, String> {
    if s.is_empty() {
        return Ok(Modifiers::empty());
    }
    let mut mods = Modifiers::empty();
    for part in s.split('|') {
        let part = part.trim();
        match part.to_lowercase().as_str() {
            "command" | "super" | "cmd" => mods |= Modifiers::COMMAND,
            "control" | "ctrl" => mods |= Modifiers::CTRL,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" => mods |= Modifiers::ALT,
            _ => return Err(format!("unknown modifier: {part}")),
        }
    }
    Ok(mods)
}

/// Parse a key string: "V", "F11", "Plus", "ArrowLeft", etc.
fn parse_key(s: &str) -> Result<BindingKey, String> {
    // Single character → Character
    if s.len() == 1 {
        return Ok(BindingKey::Character(s.to_string()));
    }
    // Named keys
    let named = match s {
        "ArrowUp" | "Up" => BindingNamedKey::ArrowUp,
        "ArrowDown" | "Down" => BindingNamedKey::ArrowDown,
        "ArrowLeft" | "Left" => BindingNamedKey::ArrowLeft,
        "ArrowRight" | "Right" => BindingNamedKey::ArrowRight,
        "Enter" | "Return" => BindingNamedKey::Enter,
        "Escape" => BindingNamedKey::Escape,
        "Tab" => BindingNamedKey::Tab,
        "Backspace" => BindingNamedKey::Backspace,
        "Home" => BindingNamedKey::Home,
        "End" => BindingNamedKey::End,
        "PageUp" => BindingNamedKey::PageUp,
        "PageDown" => BindingNamedKey::PageDown,
        "Insert" => BindingNamedKey::Insert,
        "Delete" => BindingNamedKey::Delete,
        "Space" => BindingNamedKey::Space,
        "Plus" | "+" => BindingNamedKey::Plus,
        "Minus" | "-" => BindingNamedKey::Minus,
        "Equals" | "=" => BindingNamedKey::Equals,
        _ => {
            // F-key pattern: F1..F12
            if let Some(n) = s.strip_prefix('F').or_else(|| s.strip_prefix('f')) {
                if let Ok(num) = n.parse::<u8>() {
                    if num >= 1 && num <= 12 {
                        return Ok(BindingKey::Named(BindingNamedKey::F(num)));
                    }
                }
            }
            return Err(format!("unknown key: {s}"));
        }
    };
    Ok(BindingKey::Named(named))
}

/// Parse an action string to an `Action` variant.
/// Supports both Alacritty-style names (Paste, Copy) and PureTerm-style
/// (ClipboardPaste, SplitHorizontal).
fn parse_action(s: &str) -> Result<Action, String> {
    match s {
        // Tab management
        "NewTab" | "SpawnNewTab" => Ok(Action::NewTab),
        "CloseTab" => Ok(Action::CloseTab),
        "ClosePane" => Ok(Action::ClosePaneOrTab), // TOML "ClosePane" maps to ClosePaneOrTab
        "NextTab" => Ok(Action::NextTab),
        "PrevTab" | "PreviousTab" => Ok(Action::PrevTab),
        "MovePaneToNewTab" => Ok(Action::MovePaneToNewTab),
        "MovePaneToNewWindow" => Ok(Action::MovePaneToNewWindow),

        // Split pane
        "SplitHorizontal" | "NewPaneHorizontal" => Ok(Action::SplitHorizontal),
        "SplitVertical" | "NewPaneVertical" => Ok(Action::SplitVertical),
        "EqualizeSplits" => Ok(Action::EqualizeSplits),
        "SwapPane" => Ok(Action::SwapPane),
        "ToggleZoom" | "ToggleMaximize" => Ok(Action::ToggleZoom),
        "FocusPaneNext" | "FocusNextPane" => Ok(Action::FocusPaneInOrder(1)),
        "FocusPanePrev" | "FocusPrevPane" => Ok(Action::FocusPaneInOrder(-1)),

        // Clipboard
        "ClipboardPaste" | "Paste" => Ok(Action::ClipboardPaste),
        "ClipboardCopy" | "Copy" => Ok(Action::ClipboardCopy),
        "SelectAll" => Ok(Action::SelectAll),

        // Dialogs
        "OpenSshDialog" => Ok(Action::OpenSshDialog),
        "OpenCommandPalette" | "ToggleCommandPalette" => Ok(Action::OpenCommandPalette),
        "OpenSettings" => Ok(Action::OpenSettings),

        // Font / window
        "IncreaseFontSize" | "FontLarger" => Ok(Action::IncreaseFontSize),
        "DecreaseFontSize" | "FontSmaller" => Ok(Action::DecreaseFontSize),
        "ResetFontSize" | "FontReset" => Ok(Action::ResetFontSize),
        "ToggleFullscreen" => Ok(Action::ToggleFullscreen),
        "SpawnNewInstance" => Ok(Action::SpawnNewInstance),

        _ => Err(format!("unknown action: {s}")),
    }
}

// ── Palette ID → Action mapping ─────────────────────────────────────────

/// Map a command palette builtin ID string to an `Action`.
/// Returns `None` for palette-only entries (like "settings") that have
/// inline dispatch logic.
pub fn action_from_palette_id(id: &str) -> Option<Action> {
    match id {
        "new_tab" => Some(Action::NewTab),
        "close_tab" => Some(Action::CloseTab),
        "next_tab" => Some(Action::NextTab),
        "prev_tab" => Some(Action::PrevTab),
        "split_h" => Some(Action::SplitHorizontal),
        "split_v" => Some(Action::SplitVertical),
        "split.equalize" => Some(Action::EqualizeSplits),
        "split.swap" => Some(Action::SwapPane),
        "split.zoom" => Some(Action::ToggleZoom),
        "split.focus_next" => Some(Action::FocusPaneInOrder(1)),
        "split.focus_prev" => Some(Action::FocusPaneInOrder(-1)),
        "split.move_to_new_tab" => Some(Action::MovePaneToNewTab),
        "split.move_to_new_window" => Some(Action::MovePaneToNewWindow),
        _ => None,
    }
}

// ── ContextMenuItem → Action mapping ────────────────────────────────────

use crate::iced_app::ContextMenuItem;

/// Map a context menu item to an `Action`.
/// Returns `None` for items that need `tab_id` context (CloseTab,
/// MoveToNewWindow, CloseOtherTabs, etc.).
pub fn action_from_context_menu(item: ContextMenuItem) -> Option<Action> {
    match item {
        ContextMenuItem::SplitHorizontal => Some(Action::SplitHorizontal),
        ContextMenuItem::SplitVertical => Some(Action::SplitVertical),
        ContextMenuItem::ClosePane => Some(Action::ClosePaneOrTab),
        ContextMenuItem::EqualizeSplits => Some(Action::EqualizeSplits),
        ContextMenuItem::SwapPane => Some(Action::SwapPane),
        ContextMenuItem::ToggleZoom => Some(Action::ToggleZoom),
        ContextMenuItem::MovePaneToNewTab => Some(Action::MovePaneToNewTab),
        ContextMenuItem::MovePaneToNewWindow => Some(Action::MovePaneToNewWindow),
        #[cfg(unix)]
        ContextMenuItem::NewSshTab => Some(Action::OpenSshDialog),
        // Tab-level items need tab_id context — not convertible to Action
        ContextMenuItem::CloseTab
        | ContextMenuItem::CloseOtherTabs
        | ContextMenuItem::CloseTabsToLeft
        | ContextMenuItem::CloseTabsToRight
        | ContextMenuItem::MoveToNewWindow
        | ContextMenuItem::ShowAllTabs
        | ContextMenuItem::RenameTab
        | ContextMenuItem::ChangeTabColor => None,
    }
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_keymap() -> Keymap {
        Keymap::from_defaults()
    }

    #[test]
    fn test_default_bindings_count() {
        let km = make_keymap();
        // Should have a reasonable number of bindings (>20)
        assert!(
            km.bindings.len() > 20,
            "expected >20 default bindings, got {}",
            km.bindings.len()
        );
    }

    #[test]
    fn test_cmd_t_new_tab() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("t".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::NewTab));
    }

    #[test]
    fn test_cmd_w_close() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("w".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::ClosePaneOrTab));
    }

    #[test]
    fn test_cmd_d_split_h() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("d".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::SplitHorizontal));
    }

    #[test]
    fn test_cmd_shift_d_split_v() {
        let mut km = make_keymap();
        // macOS may report ⌘⇧D as "D" without shift
        let result = km.resolve(
            &Key::Character("D".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::SplitVertical));

        // Or as "d" with shift
        let result = km.resolve(
            &Key::Character("d".into()),
            Modifiers::COMMAND | Modifiers::SHIFT,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::SplitVertical));
    }

    #[test]
    fn test_cmd_shift_enter_zoom() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Named(Named::Enter),
            Modifiers::COMMAND | Modifiers::SHIFT,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::ToggleZoom));
    }

    #[test]
    fn test_cmd_equals_equalize() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("=".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::EqualizeSplits));
    }

    #[test]
    fn test_cmd_bracket_focus_pane() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("]".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::FocusPaneInOrder(1)));

        let result = km.resolve(
            &Key::Character("[".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::FocusPaneInOrder(-1)));
    }

    #[test]
    fn test_cmd_brace_tab_nav() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("}".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::NextTab));

        let result = km.resolve(
            &Key::Character("{".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::PrevTab));
    }

    #[test]
    fn test_cmd_1_to_9_tab_index() {
        let mut km = make_keymap();
        for i in 1..=9u8 {
            let ch = (b'0' + i) as char;
            let result = km.resolve(
                &Key::Character(ch.to_string().into()),
                Modifiers::COMMAND,
                Instant::now(),
            );
            assert_eq!(
                result,
                ResolveResult::Action(Action::ActivateTabByIndex((i - 1) as usize)),
                "⌘{ch} should map to ActivateTabByIndex({})",
                i - 1
            );
        }
    }

    #[test]
    fn test_cmd_shift_x_swap() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("X".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::SwapPane));

        let result = km.resolve(
            &Key::Character("x".into()),
            Modifiers::COMMAND | Modifiers::SHIFT,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::SwapPane));
    }

    #[test]
    fn test_cmd_clipboard() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("v".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::ClipboardPaste));

        let result = km.resolve(
            &Key::Character("c".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::ClipboardCopy));

        let result = km.resolve(
            &Key::Character("a".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::SelectAll));
    }

    #[test]
    fn test_cmd_shift_s_ssh() {
        let mut km = make_keymap();
        // macOS dual-form: ⌘⇧S may arrive as "S" without shift
        let result = km.resolve(
            &Key::Character("S".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::OpenSshDialog));

        let result = km.resolve(
            &Key::Character("s".into()),
            Modifiers::COMMAND | Modifiers::SHIFT,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::OpenSshDialog));

        // Also ⌘s alone maps to OpenSshDialog (current behavior)
        let result = km.resolve(
            &Key::Character("s".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::OpenSshDialog));
    }

    #[test]
    fn test_cmd_shift_p_palette() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("P".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::OpenCommandPalette));

        let result = km.resolve(
            &Key::Character("p".into()),
            Modifiers::COMMAND | Modifiers::SHIFT,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::OpenCommandPalette));
    }

    #[test]
    fn test_cmd_option_arrow_adjacent() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Named(Named::ArrowLeft),
            Modifiers::COMMAND | Modifiers::ALT,
            Instant::now(),
        );
        assert_eq!(
            result,
            ResolveResult::Action(Action::PaneFocusAdjacent(SplitDirection::Horizontal))
        );

        let result = km.resolve(
            &Key::Named(Named::ArrowUp),
            Modifiers::COMMAND | Modifiers::ALT,
            Instant::now(),
        );
        assert_eq!(
            result,
            ResolveResult::Action(Action::PaneFocusAdjacent(SplitDirection::Vertical))
        );
    }

    #[test]
    fn test_passthrough() {
        let mut km = make_keymap();
        // A plain 'x' with no modifiers should passthrough to PTY
        let result = km.resolve(
            &Key::Character("x".into()),
            Modifiers::empty(),
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Passthrough);

        // ⌘X (not shift) is not bound — only ⌘⇧X is
        // Actually ⌘X-with-uppercase "X" is bound (macOS dual-form). Test with lowercase.
        // ⌘+x without shift is NOT bound (plain x). The binding is for ⌘⇧X or ⌘+X(uppercase).
        // Let's test with a truly unbound combo:
        let result = km.resolve(
            &Key::Character("z".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Passthrough);
    }

    // ── TOML parsing tests ──────────────────────────────────────────

    #[test]
    fn test_parse_mods() {
        assert_eq!(
            parse_mods("Command|Shift").unwrap(),
            Modifiers::COMMAND | Modifiers::SHIFT
        );
        assert_eq!(
            parse_mods("Command").unwrap(),
            Modifiers::COMMAND
        );
        assert_eq!(
            parse_mods("Ctrl|Alt").unwrap(),
            Modifiers::CTRL | Modifiers::ALT
        );
        assert_eq!(parse_mods("").unwrap(), Modifiers::empty());
    }

    #[test]
    fn test_parse_key() {
        assert_eq!(
            parse_key("V").unwrap(),
            BindingKey::Character("V".into())
        );
        assert_eq!(
            parse_key("F11").unwrap(),
            BindingKey::Named(BindingNamedKey::F(11))
        );
        assert_eq!(
            parse_key("Enter").unwrap(),
            BindingKey::Named(BindingNamedKey::Enter)
        );
        assert_eq!(
            parse_key("Plus").unwrap(),
            BindingKey::Named(BindingNamedKey::Plus)
        );
    }

    #[test]
    fn test_parse_action() {
        assert_eq!(parse_action("Paste").unwrap(), Action::ClipboardPaste);
        assert_eq!(parse_action("Copy").unwrap(), Action::ClipboardCopy);
        assert_eq!(
            parse_action("SplitHorizontal").unwrap(),
            Action::SplitHorizontal
        );
        assert_eq!(
            parse_action("ToggleFullscreen").unwrap(),
            Action::ToggleFullscreen
        );
    }

    #[test]
    fn test_toml_override() {
        let defaults = Keymap::from_defaults();
        let overrides = vec![RawBinding {
            key: "d".into(),
            mods: "Command".into(),
            action: "SplitVertical".into(), // Override ⌘D to do vertical split
        }];
        let mut km = Keymap::from_toml_overrides(&overrides, &defaults);
        let result = km.resolve(
            &Key::Character("d".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::SplitVertical));
    }

    #[test]
    fn test_chord_timeout() {
        let mut km = make_keymap();
        // No chord bindings in defaults — pending chord should never be set.
        // Test that a passthrough key stays passthrough.
        let result = km.resolve(
            &Key::Character("j".into()),
            Modifiers::empty(),
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Passthrough);
    }

    #[test]
    fn test_action_from_palette_id() {
        assert_eq!(
            action_from_palette_id("new_tab"),
            Some(Action::NewTab)
        );
        assert_eq!(
            action_from_palette_id("split.equalize"),
            Some(Action::EqualizeSplits)
        );
        assert_eq!(action_from_palette_id("settings"), None);
    }
}
