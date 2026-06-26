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

    // ── Workspace (GAP-2.1) ────────────────────────────────────────
    /// Create a new named workspace, snapshot the current one into the
    /// background, and switch to it (opens a fresh shell tab).
    NewWorkspace,
    /// Switch to the next workspace (wraps around).
    NextWorkspace,
    /// Switch to the previous workspace (wraps around).
    PrevWorkspace,
    /// Switch to workspace by 0-based index (⌃1..9 → 0..8).
    ActivateWorkspaceByIndex(usize),

    // ── Split pane ──────────────────────────────────────────────────
    SplitHorizontal,
    SplitVertical,
    /// GAP-3.5: split a new Web pane (wry WebView) beside the active pane,
    /// then focus the ⌘L address bar so the user types a URL immediately.
    SplitWeb,
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
    /// GAP-3.5: focus the Browser address-bar overlay (⌘L). Navigates the
    /// active Web pane, or spawns one if no Web pane is focused.
    FocusAddressBar,
    /// GAP-32: toggle the Git overlay panel (⌘⇧B).
    ToggleGitPanel,
    /// GAP-32: open lazygit in the active pane (panel button / command palette).
    OpenLazygit,

    // ── Font / window (TOML-declared but not yet wired) ─────────────
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    ToggleFullscreen,
    SpawnNewInstance,

    // ── Vi / Copy mode ───────────────────────────────────────────────
    /// Toggle Vi (copy) mode on the active pane.
    /// When active, keystrokes are interpreted as vi motions instead of
    /// being forwarded to the PTY.
    ToggleViMode,

    // ── Search (GAP-3.8 / GAP-4.3) ───────────────────────────────────
    /// ⌘F — open the current-surface search overlay on the active pane.
    SearchForward,
    /// ⌘⇧F — open the working-directory (ripgrep) search overlay.
    SearchInDirectory,
    /// F3 / ⌘G — jump to the next match (or next ripgrep result).
    SearchNext,
    /// ⇧F3 / ⌘⇧G — jump to the previous match.
    SearchPrev,
    /// Escape / ⌘F again — close the active search overlay.
    SearchClose,
    /// Toggle case-sensitive flag.
    SearchToggleCase,
    /// Toggle regex mode flag.
    SearchToggleRegex,
    /// Toggle whole-word flag.
    SearchToggleWholeWord,
    /// GAP-4.3: toggle follow-tail (auto-scroll on new output).
    SearchToggleFollow,

    // ── User-defined commands (GAP-6.4) ───────────────────────────
    /// Run a user-defined `[[commands]]` entry by id. Resolved to a command
    /// string at dispatch time and written into the active pane's PTY
    /// (see `iced_app::dispatch_action`).
    CustomCommand(String),
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
        let ctrl = Modifiers::CTRL;
        let cmd_ctrl = Modifiers::COMMAND | Modifiers::CTRL;

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

        // ── Workspace (GAP-2.1) ────────────────────────────────────
        // ⌃1..9 → ActivateWorkspaceByIndex(0..8). ⌘1..9 is tabs, so workspace
        // switching rides the Control modifier (i3-style).
        for i in 1..=9u8 {
            let ch = (b'0' + i) as char;
            bindings.insert(
                (ctrl, BindingKey::Character(ch.to_string())),
                Action::ActivateWorkspaceByIndex((i - 1) as usize),
            );
        }
        // ⌘⌃← / ⌘⌃→ → Prev / Next workspace.
        bindings.insert(
            (cmd_ctrl, BindingKey::Named(BindingNamedKey::ArrowLeft)),
            Action::PrevWorkspace,
        );
        bindings.insert(
            (cmd_ctrl, BindingKey::Named(BindingNamedKey::ArrowRight)),
            Action::NextWorkspace,
        );
        // ⌘⌃N → New workspace.
        bindings.insert(
            (cmd_ctrl, BindingKey::Character("n".into())),
            Action::NewWorkspace,
        );

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
        // GAP-3.5: ⌘⇧L → SplitWeb (split a new Web pane + focus address bar).
        // macOS dual-form: "l" with shift, or "L" without (mirrors ⌘⇧D above).
        bindings.insert(
            (cmd_shift, BindingKey::Character("l".into())),
            Action::SplitWeb,
        );
        bindings.insert((cmd, BindingKey::Character("L".into())), Action::SplitWeb);
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

        // ── Browser (GAP-3.5) ──────────────────────────────────────
        // ⌘L → FocusAddressBar (navigate the active Web pane / spawn one).
        bindings.insert(
            (cmd, BindingKey::Character("l".into())),
            Action::FocusAddressBar,
        );

        // ── Git panel (GAP-32) ─────────────────────────────────────
        // ⌘⇧B → ToggleGitPanel. macOS dual-form (⌘⇧B may report as ⌘B-uppercase).
        // Plain ⌘B is intentionally left free.
        bindings.insert(
            (cmd_shift, BindingKey::Character("b".into())),
            Action::ToggleGitPanel,
        );
        bindings.insert(
            (cmd, BindingKey::Character("B".into())),
            Action::ToggleGitPanel,
        );

        // ── Cmd+Option+Arrow → PaneFocusAdjacent ────────────────────
        // Ctrl+Shift+Space → ToggleViMode (copy mode)
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        bindings.insert(
            (ctrl_shift, BindingKey::Named(BindingNamedKey::Space)),
            Action::ToggleViMode,
        );

        // ── Search (GAP-3.8 / GAP-4.3) ──────────────────────────────
        // ⌘F → open current-surface search overlay.
        bindings.insert(
            (cmd, BindingKey::Character("f".into())),
            Action::SearchForward,
        );
        // ⌘⇧F → working-directory (ripgrep) search. macOS dual-form.
        bindings.insert(
            (cmd_shift, BindingKey::Character("f".into())),
            Action::SearchInDirectory,
        );
        bindings.insert(
            (cmd, BindingKey::Character("F".into())),
            Action::SearchInDirectory,
        );
        // F3 → next match, ⇧F3 → prev (iced delivers F-keys as Character).
        bindings.insert(
            (Modifiers::empty(), BindingKey::Character("F3".into())),
            Action::SearchNext,
        );
        bindings.insert(
            (Modifiers::SHIFT, BindingKey::Character("F3".into())),
            Action::SearchPrev,
        );
        // ⌘G / ⌘⇧G → next / prev match (macOS alternative to F3).
        bindings.insert(
            (cmd, BindingKey::Character("g".into())),
            Action::SearchNext,
        );
        bindings.insert(
            (cmd_shift, BindingKey::Character("g".into())),
            Action::SearchPrev,
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

    /// GAP-6.4: register user-defined `[[commands]]` as keymap bindings.
    ///
    /// Each command with a `key` becomes an `Action::CustomCommand(id)`
    /// binding. Commands without a `key` are palette-only and skipped here.
    ///
    /// Letter-key normalization (mirrors how the default bindings are stored
    /// so user shortcuts match the actual key events the OS delivers):
    /// - **Shift + letter** → register both forms, since macOS may report
    ///   the event as lowercase+shift OR uppercase without shift;
    /// - **no Shift + letter** → store lowercase (the OS reports unshifted
    ///   letters as lowercase), regardless of the case the user wrote.
    pub fn register_custom_commands(&mut self, cmds: &[CustomCommand]) {
        for c in cmds {
            let Some(key_str) = c.key.as_deref() else {
                continue;
            };
            let (mods, key) = split_mods_and_key(key_str, c.mods.as_deref());
            let (Some(mods), Some(key)) = (mods, key) else {
                continue;
            };

            let action = Action::CustomCommand(c.id.clone());

            // Letter-key normalization (see method doc).
            if let BindingKey::Character(ch) = &key {
                if ch.len() == 1 && ch.chars().next().unwrap().is_ascii_alphabetic() {
                    let c0 = ch.chars().next().unwrap();
                    if mods.contains(Modifiers::SHIFT) {
                        // macOS dual-form: report lowercase+shift OR uppercase
                        // without shift. Register both so the binding fires
                        // regardless of which form the OS delivers.
                        let lower = c0.to_ascii_lowercase().to_string();
                        let upper = c0.to_ascii_uppercase().to_string();
                        self.bindings
                            .insert((mods, BindingKey::Character(lower)), action.clone());
                        self.bindings.insert(
                            (mods ^ Modifiers::SHIFT, BindingKey::Character(upper)),
                            action,
                        );
                        continue;
                    } else {
                        // Unshifted letter: the OS reports it lowercase, so
                        // store lowercase to match (mirrors the defaults).
                        let lower = c0.to_ascii_lowercase().to_string();
                        self.bindings
                            .insert((mods, BindingKey::Character(lower)), action);
                        continue;
                    }
                }
            }
            self.bindings.insert((mods, key), action);
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

/// Raw `[[commands]]` entry as read from TOML (GAP-6.4). Defined here
/// (alongside `RawBinding`) so `app_config::load_custom_commands` can
/// return `Vec<CustomCommand>` without creating an `app_config → keymap`
/// dependency cycle — this mirrors how `load_keybindings` returns
/// `Vec<RawBinding>`.
///
/// The `key` field, when present, is a combined "Mods+Key" string (e.g.
/// `"Cmd+Shift+D"`) or a bare key (e.g. `"F5"`); `mods` optionally overrides
/// the modifier portion (in which case `key` is parsed bare). Parsed into a
/// `Keymap` binding via `Keymap::register_custom_commands`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomCommand {
    pub id: String,
    pub title: String,
    pub command: String,
    pub key: Option<String>,
    pub mods: Option<String>,
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

/// Split a combined "Mods+Key" string (GAP-6.4) into parsed `(Modifiers,
/// BindingKey)`. Accepts both `+` and `|` as separators; the final segment
/// is the key, preceding segments are modifiers.
///
/// If `explicit_mods` is `Some`, it is used for the modifier portion and
/// `key_str` is parsed as a bare key (no splitting). This supports a
/// separate `mods = "Command|Shift"` + `key = "D"` TOML form.
///
/// Returns `(None, _)` / `(_, None)` on parse failure (logged via `warn`);
/// the caller skips the binding.
fn split_mods_and_key(
    key_str: &str,
    explicit_mods: Option<&str>,
) -> (Option<Modifiers>, Option<BindingKey>) {
    let (mods_raw, key_raw) = match explicit_mods {
        Some(em) => (em.to_string(), key_str.trim().to_string()),
        None => {
            // Split off the final segment after the last separator.
            match key_str.rfind(|c| c == '+' || c == '|') {
                Some(pos) => (
                    key_str[..pos].to_string(),
                    key_str[pos + 1..].trim().to_string(),
                ),
                None => (String::new(), key_str.trim().to_string()),
            }
        }
    };
    // Normalize '+' → '|' so parse_mods (which splits on '|') accepts both.
    let mods_str = mods_raw.replace('+', "|");
    let mods = match parse_mods(&mods_str) {
        Ok(m) => Some(m),
        Err(e) => {
            log::warn!("custom command: bad mods '{}': {}", mods_raw, e);
            None
        }
    };
    let key = match parse_key(&key_raw) {
        Ok(k) => Some(k),
        Err(e) => {
            log::warn!("custom command: bad key '{}': {}", key_raw, e);
            None
        }
    };
    (mods, key)
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

        // Workspace (GAP-2.1)
        "NewWorkspace" => Ok(Action::NewWorkspace),
        "NextWorkspace" => Ok(Action::NextWorkspace),
        "PrevWorkspace" | "PreviousWorkspace" => Ok(Action::PrevWorkspace),
        "ActivateWorkspaceByIndex" => Ok(Action::ActivateWorkspaceByIndex(0)),

        // Split pane
        "SplitHorizontal" | "NewPaneHorizontal" => Ok(Action::SplitHorizontal),
        "SplitVertical" | "NewPaneVertical" => Ok(Action::SplitVertical),
        "SplitWeb" | "SplitPaneWeb" => Ok(Action::SplitWeb),
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
        // Browser (GAP-3.5)
        "FocusAddressBar" | "FocusUrlBar" => Ok(Action::FocusAddressBar),
        // Git panel (GAP-32)
        "ToggleGitPanel" | "GitPanel" => Ok(Action::ToggleGitPanel),
        "OpenLazygit" | "Lazygit" => Ok(Action::OpenLazygit),

        // Font / window
        "IncreaseFontSize" | "FontLarger" => Ok(Action::IncreaseFontSize),
        "DecreaseFontSize" | "FontSmaller" => Ok(Action::DecreaseFontSize),
        "ResetFontSize" | "FontReset" => Ok(Action::ResetFontSize),
        "ToggleFullscreen" => Ok(Action::ToggleFullscreen),
        "SpawnNewInstance" => Ok(Action::SpawnNewInstance),

        // Vi / Copy mode
        "ToggleViMode" | "ViMode" | "CopyMode" => Ok(Action::ToggleViMode),

        // Search (GAP-3.8 / GAP-4.3)
        "SearchForward" | "Find" => Ok(Action::SearchForward),
        "SearchInDirectory" | "FindInDirectory" => Ok(Action::SearchInDirectory),
        "SearchNext" | "FindNext" => Ok(Action::SearchNext),
        "SearchPrev" | "FindPrevious" | "FindPrev" => Ok(Action::SearchPrev),
        "SearchClose" => Ok(Action::SearchClose),
        "SearchToggleCase" => Ok(Action::SearchToggleCase),
        "SearchToggleRegex" => Ok(Action::SearchToggleRegex),
        "SearchToggleWholeWord" => Ok(Action::SearchToggleWholeWord),
        "SearchToggleFollow" => Ok(Action::SearchToggleFollow),

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
        "split_web" => Some(Action::SplitWeb),
        "browser.focus_address" => Some(Action::FocusAddressBar),
        "split.equalize" => Some(Action::EqualizeSplits),
        "split.swap" => Some(Action::SwapPane),
        "split.zoom" => Some(Action::ToggleZoom),
        "split.focus_next" => Some(Action::FocusPaneInOrder(1)),
        "split.focus_prev" => Some(Action::FocusPaneInOrder(-1)),
        "split.move_to_new_tab" => Some(Action::MovePaneToNewTab),
        "split.move_to_new_window" => Some(Action::MovePaneToNewWindow),
        "workspace.new" => Some(Action::NewWorkspace),
        "workspace.next" => Some(Action::NextWorkspace),
        "workspace.prev" => Some(Action::PrevWorkspace),
        "toggle_vi_mode" | "toggle_copy_mode" => Some(Action::ToggleViMode),
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

    // ── GAP-3.5: Browser address bar + split-web bindings ────────────

    #[test]
    fn test_cmd_l_focus_address_bar() {
        let mut km = make_keymap();
        let result = km.resolve(
            &Key::Character("l".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(result, ResolveResult::Action(Action::FocusAddressBar));
    }

    #[test]
    fn test_cmd_shift_l_split_web_dual_form() {
        let mut km = make_keymap();
        // macOS may report ⌘⇧L as "L" without shift…
        let r1 = km.resolve(
            &Key::Character("L".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(r1, ResolveResult::Action(Action::SplitWeb));
        // …or as "l" with shift.
        let r2 = km.resolve(
            &Key::Character("l".into()),
            Modifiers::COMMAND | Modifiers::SHIFT,
            Instant::now(),
        );
        assert_eq!(r2, ResolveResult::Action(Action::SplitWeb));
    }

    #[test]
    fn test_parse_browser_actions() {
        assert_eq!(parse_action("FocusAddressBar"), Ok(Action::FocusAddressBar));
        assert_eq!(parse_action("FocusUrlBar"), Ok(Action::FocusAddressBar));
        assert_eq!(parse_action("SplitWeb"), Ok(Action::SplitWeb));
        assert_eq!(parse_action("SplitPaneWeb"), Ok(Action::SplitWeb));
    }

    // ── GAP-3.8 / GAP-4.3: search action parsing + bindings ──────────

    #[test]
    fn test_parse_search_actions() {
        // Canonical names
        assert_eq!(parse_action("SearchForward"), Ok(Action::SearchForward));
        assert_eq!(parse_action("SearchInDirectory"), Ok(Action::SearchInDirectory));
        assert_eq!(parse_action("SearchNext"), Ok(Action::SearchNext));
        assert_eq!(parse_action("SearchPrev"), Ok(Action::SearchPrev));
        assert_eq!(parse_action("SearchClose"), Ok(Action::SearchClose));
        assert_eq!(parse_action("SearchToggleCase"), Ok(Action::SearchToggleCase));
        assert_eq!(parse_action("SearchToggleRegex"), Ok(Action::SearchToggleRegex));
        assert_eq!(parse_action("SearchToggleWholeWord"), Ok(Action::SearchToggleWholeWord));
        assert_eq!(parse_action("SearchToggleFollow"), Ok(Action::SearchToggleFollow));
        // iTerm2/Alacritty-style aliases
        assert_eq!(parse_action("Find"), Ok(Action::SearchForward));
        assert_eq!(parse_action("FindInDirectory"), Ok(Action::SearchInDirectory));
        assert_eq!(parse_action("FindNext"), Ok(Action::SearchNext));
        assert_eq!(parse_action("FindPrevious"), Ok(Action::SearchPrev));
    }

    #[test]
    fn test_default_search_bindings() {
        let mut km = make_keymap();
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;

        // ⌘F → SearchForward
        assert_eq!(
            km.resolve(&Key::Character("f".into()), cmd, Instant::now()),
            ResolveResult::Action(Action::SearchForward),
        );
        // ⌘⇧F → SearchInDirectory (macOS dual-form: "f" with shift, or "F" without)
        assert_eq!(
            km.resolve(&Key::Character("f".into()), cmd_shift, Instant::now()),
            ResolveResult::Action(Action::SearchInDirectory),
        );
        assert_eq!(
            km.resolve(&Key::Character("F".into()), cmd, Instant::now()),
            ResolveResult::Action(Action::SearchInDirectory),
        );
        // F3 → SearchNext, ⇧F3 → SearchPrev (iced delivers F-keys as Character("F3"))
        assert_eq!(
            km.resolve(&Key::Character("F3".into()), Modifiers::empty(), Instant::now()),
            ResolveResult::Action(Action::SearchNext),
        );
        assert_eq!(
            km.resolve(&Key::Character("F3".into()), Modifiers::SHIFT, Instant::now()),
            ResolveResult::Action(Action::SearchPrev),
        );
        // ⌘G → SearchNext, ⌘⇧G → SearchPrev
        assert_eq!(
            km.resolve(&Key::Character("g".into()), cmd, Instant::now()),
            ResolveResult::Action(Action::SearchNext),
        );
        assert_eq!(
            km.resolve(&Key::Character("g".into()), cmd_shift, Instant::now()),
            ResolveResult::Action(Action::SearchPrev),
        );
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

    // ── GAP-6.4 custom command tests ───────────────────────────────

    #[test]
    fn test_split_mods_and_key_plus_separator() {
        let (mods, key) = split_mods_and_key("Cmd+Shift+D", None);
        assert_eq!(mods, Some(Modifiers::COMMAND | Modifiers::SHIFT));
        assert_eq!(key, Some(BindingKey::Character("D".into())));
    }

    #[test]
    fn test_split_mods_and_key_pipe_separator() {
        let (mods, key) = split_mods_and_key("Command|Shift|D", None);
        assert_eq!(mods, Some(Modifiers::COMMAND | Modifiers::SHIFT));
        assert_eq!(key, Some(BindingKey::Character("D".into())));
    }

    #[test]
    fn test_split_mods_and_key_no_mods() {
        let (mods, key) = split_mods_and_key("F5", None);
        assert_eq!(mods, Some(Modifiers::empty()));
        assert_eq!(key, Some(BindingKey::Named(BindingNamedKey::F(5))));
    }

    #[test]
    fn test_split_mods_and_key_explicit_mods() {
        // When explicit mods are given, key_str is parsed as a bare key.
        let (mods, key) = split_mods_and_key("D", Some("Alt"));
        assert_eq!(mods, Some(Modifiers::ALT));
        assert_eq!(key, Some(BindingKey::Character("D".into())));
    }

    #[test]
    fn test_register_custom_command_basic() {
        // Ctrl+B is not bound by defaults, so it must resolve to the
        // registered custom command.
        let mut km = make_keymap();
        km.register_custom_commands(&[CustomCommand {
            id: "cmd.basic".into(),
            title: "Basic".into(),
            command: "echo hi".into(),
            key: Some("Ctrl+B".into()),
            mods: None,
        }]);
        let r = km.resolve(
            &Key::Character("b".into()),
            Modifiers::CTRL,
            Instant::now(),
        );
        assert_eq!(
            r,
            ResolveResult::Action(Action::CustomCommand("cmd.basic".into()))
        );
    }

    #[test]
    fn test_register_custom_command_shift_dual_form() {
        // "Cmd+Shift+Q" must fire whether macOS reports it as
        // lowercase 'q' with CMD|SHIFT, or uppercase 'Q' with CMD.
        let mut km = make_keymap();
        km.register_custom_commands(&[CustomCommand {
            id: "cmd.shift".into(),
            title: "Shift".into(),
            command: "echo shift".into(),
            key: Some("Cmd+Shift+Q".into()),
            mods: None,
        }]);
        let r1 = km.resolve(
            &Key::Character("q".into()),
            Modifiers::COMMAND | Modifiers::SHIFT,
            Instant::now(),
        );
        assert_eq!(
            r1,
            ResolveResult::Action(Action::CustomCommand("cmd.shift".into()))
        );
        let r2 = km.resolve(
            &Key::Character("Q".into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(
            r2,
            ResolveResult::Action(Action::CustomCommand("cmd.shift".into()))
        );
    }

    #[test]
    fn test_register_custom_command_no_key_skipped() {
        // A command without a `key` is palette-only; nothing is registered.
        let mut km = make_keymap();
        let before = km.bindings.len();
        km.register_custom_commands(&[CustomCommand {
            id: "cmd.nokey".into(),
            title: "NoKey".into(),
            command: "echo".into(),
            key: None,
            mods: None,
        }]);
        assert_eq!(km.bindings.len(), before);
    }
}
