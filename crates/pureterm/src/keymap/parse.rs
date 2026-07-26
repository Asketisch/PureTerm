//! TOML 绑定解析 — `RawBinding` / `CustomCommand` 结构体与
//! `parse_mods` / `parse_key` / `parse_action` / `split_mods_and_key` 辅助函数。
//!
//! 支持 Alacritty 风格名（Paste/Copy）与 PureTerm 风格名（ClipboardPaste/
//! SplitHorizontal），以及 `+` / `|` 两种修饰键分隔符。

use iced::keyboard::Modifiers;

use super::action::Action;
use super::types::{BindingKey, BindingNamedKey};

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

pub(crate) fn parse_raw_binding(
    rb: &RawBinding,
) -> Result<((Modifiers, BindingKey), Action), String> {
    let mods = parse_mods(&rb.mods)?;
    let key = parse_key(&rb.key)?;
    let action = parse_action(&rb.action)?;
    Ok(((mods, key), action))
}

/// Parse pipe-separated modifiers: "Command|Shift", "Control", "Alt|Shift"
pub(crate) fn parse_mods(s: &str) -> Result<Modifiers, String> {
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
pub(crate) fn parse_key(s: &str) -> Result<BindingKey, String> {
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
pub(crate) fn split_mods_and_key(
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
pub(crate) fn parse_action(s: &str) -> Result<Action, String> {
    match s {
        // Tab management
        "NewTab" | "SpawnNewTab" => Ok(Action::NewTab),
        "CloseTab" => Ok(Action::CloseTab),
        "ClosePane" => Ok(Action::ClosePaneOrTab), // TOML "ClosePane" maps to ClosePaneOrTab
        "NextTab" => Ok(Action::NextTab),
        "PrevTab" | "PreviousTab" => Ok(Action::PrevTab),
        "MovePaneToNewTab" => Ok(Action::MovePaneToNewTab),
        "MovePaneToNewWindow" => Ok(Action::MovePaneToNewWindow),
        "NewWindow" => Ok(Action::NewWindow),

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
        "ToggleLeftSidebar" | "LeftSidebar" => Ok(Action::ToggleLeftSidebar),
        "ToggleRightSidebar" | "RightSidebar" => Ok(Action::ToggleRightSidebar),
        "OpenLazygit" | "Lazygit" => Ok(Action::OpenLazygit),
        // Notifications panel (GAP-09)
        "ToggleNotificationsPanel" | "NotificationsPanel" => Ok(Action::ToggleNotificationsPanel),
        "ErrorRecoveryToggle" | "error_recovery.toggle" => Ok(Action::ErrorRecoveryToggle),
        "ToggleAiChatPanel" | "AiChatPanel" | "ai_chat.toggle" => Ok(Action::ToggleAiChatPanel),
        "OpenHoverTarget" | "hover_preview.open" => Ok(Action::OpenHoverTarget),
        "OpenSemanticTarget" | "semantic_history.open" => Ok(Action::OpenSemanticInEditor),
        "PreviewFile" | "file_preview.preview" => Ok(Action::PreviewFile),
        "ToggleInstantReplay" | "instant_replay.toggle" => Ok(Action::ToggleInstantReplay),
        "ToggleBroadcastInput" | "broadcast.toggle" => Ok(Action::ToggleBroadcastInput),
        "ToggleQuakeDock" | "quake.toggle" => Ok(Action::ToggleQuakeDock),
        "ToggleToolbelt" | "toolbelt.toggle" => Ok(Action::ToggleToolbelt),
        "PrevMark" | "marks.prev" => Ok(Action::PrevMark),
        "NextMark" | "marks.next" => Ok(Action::NextMark),

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
