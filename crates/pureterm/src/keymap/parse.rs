//! TOML 绑定解析 — `RawBinding` / `CustomCommand` 结构体与
//! `parse_mods` / `parse_key` / `parse_action` / `split_mods_and_key` 辅助函数。
//!
//! 支持 Alacritty 风格名（Paste/Copy）与 PureTerm 风格名（ClipboardPaste/
//! SplitHorizontal），以及 `+` / `|` 两种修饰键分隔符。

use iced::keyboard::Modifiers;

use super::action::Action;
use super::types::{BindingKey, BindingNamedKey};

// ── TOML 原始绑定 ────────────────────────────────────────────────

/// 从 `[[keyboard.bindings]]` TOML 段读取的原始绑定条目。
#[derive(Debug, Clone)]
pub struct RawBinding {
    pub key: String,
    pub mods: String,
    pub action: String,
}

/// 从 TOML 读取的原始 `[[commands]]` 条目(GAP-6.4)。与 `RawBinding` 一起
/// 定义在这里,使 `app_config::load_custom_commands` 能返回
/// `Vec<CustomCommand>`,而不产生 `app_config → keymap` 的依赖环 —— 这与
/// `load_keybindings` 返回 `Vec<RawBinding>` 的方式一致。
///
/// `key` 字段(若存在)是一个组合的 "Mods+Key" 字符串(如 `"Cmd+Shift+D"`)
/// 或裸键(如 `"F5"`);`mods` 可覆盖修饰键部分(此时 `key` 按裸键解析)。
/// 通过 `Keymap::register_custom_commands` 解析为 `Keymap` 绑定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomCommand {
    pub id: String,
    pub title: String,
    pub command: String,
    pub key: Option<String>,
    pub mods: Option<String>,
}

// ── TOML 解析辅助 ────────────────────────────────────────────────

pub(crate) fn parse_raw_binding(
    rb: &RawBinding,
) -> Result<((Modifiers, BindingKey), Action), String> {
    let mods = parse_mods(&rb.mods)?;
    let key = parse_key(&rb.key)?;
    let action = parse_action(&rb.action)?;
    Ok(((mods, key), action))
}

/// 解析以 | 分隔的修饰键:"Command|Shift"、"Control"、"Alt|Shift"
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

/// 解析键字符串:"V"、"F11"、"Plus"、"ArrowLeft" 等。
pub(crate) fn parse_key(s: &str) -> Result<BindingKey, String> {
    // 单个字符 → Character
    if s.len() == 1 {
        return Ok(BindingKey::Character(s.to_string()));
    }
    // 命名键
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
            // F 键模式:F1..F12
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

/// 把组合的 "Mods+Key" 字符串(GAP-6.4)拆分为已解析的 `(Modifiers, BindingKey)`。
/// 同时接受 `+` 和 `|` 作为分隔符;最后一段是键,前面的段是修饰键。
///
/// 若 `explicit_mods` 为 `Some`,则用它作为修饰键部分,`key_str` 按裸键解析
/// (不拆分)。这支持 `mods = "Command|Shift"` + `key = "D"` 分开写的 TOML 形式。
///
/// 解析失败时返回 `(None, _)` / `(_, None)`(通过 `warn` 记录);调用方跳过该绑定。
pub(crate) fn split_mods_and_key(
    key_str: &str,
    explicit_mods: Option<&str>,
) -> (Option<Modifiers>, Option<BindingKey>) {
    let (mods_raw, key_raw) = match explicit_mods {
        Some(em) => (em.to_string(), key_str.trim().to_string()),
        None => {
            // 截取最后一个分隔符之后的段。
            match key_str.rfind(|c| c == '+' || c == '|') {
                Some(pos) => (
                    key_str[..pos].to_string(),
                    key_str[pos + 1..].trim().to_string(),
                ),
                None => (String::new(), key_str.trim().to_string()),
            }
        }
    };
    // 将 `+` 归一化为 `|`,使 parse_mods(按 `|` 拆分)能同时接受两种写法。
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

/// 把动作字符串解析为 `Action` 变体。
/// 同时支持 Alacritty 风格名(Paste, Copy)和 PureTerm 风格名(ClipboardPaste, SplitHorizontal)。
pub(crate) fn parse_action(s: &str) -> Result<Action, String> {
    match s {
        // 标签页管理
        "NewTab" | "SpawnNewTab" => Ok(Action::NewTab),
        "CloseTab" => Ok(Action::CloseTab),
        "ClosePane" => Ok(Action::ClosePaneOrTab), // TOML 的 "ClosePane" 映射到 ClosePaneOrTab
        "NextTab" => Ok(Action::NextTab),
        "PrevTab" | "PreviousTab" => Ok(Action::PrevTab),
        "MovePaneToNewTab" => Ok(Action::MovePaneToNewTab),
        "MovePaneToNewWindow" => Ok(Action::MovePaneToNewWindow),
        "NewWindow" => Ok(Action::NewWindow),

        // 工作区 Workspace(GAP-2.1)
        "NewWorkspace" => Ok(Action::NewWorkspace),
        "NextWorkspace" => Ok(Action::NextWorkspace),
        "PrevWorkspace" | "PreviousWorkspace" => Ok(Action::PrevWorkspace),
        "ActivateWorkspaceByIndex" => Ok(Action::ActivateWorkspaceByIndex(0)),

        // 面板分割
        "SplitHorizontal" | "NewPaneHorizontal" => Ok(Action::SplitHorizontal),
        "SplitVertical" | "NewPaneVertical" => Ok(Action::SplitVertical),
        "SplitWeb" | "SplitPaneWeb" => Ok(Action::SplitWeb),
        "EqualizeSplits" => Ok(Action::EqualizeSplits),
        "SwapPane" => Ok(Action::SwapPane),
        "ToggleZoom" | "ToggleMaximize" => Ok(Action::ToggleZoom),
        "FocusPaneNext" | "FocusNextPane" => Ok(Action::FocusPaneInOrder(1)),
        "FocusPanePrev" | "FocusPrevPane" => Ok(Action::FocusPaneInOrder(-1)),

        // 剪贴板
        "ClipboardPaste" | "Paste" => Ok(Action::ClipboardPaste),
        "ClipboardCopy" | "Copy" => Ok(Action::ClipboardCopy),
        "SelectAll" => Ok(Action::SelectAll),

        // 对话框
        "OpenSshDialog" => Ok(Action::OpenSshDialog),
        "OpenCommandPalette" | "ToggleCommandPalette" => Ok(Action::OpenCommandPalette),
        "OpenSettings" => Ok(Action::OpenSettings),
        // 浏览器 (GAP-3.5)
        "FocusAddressBar" | "FocusUrlBar" => Ok(Action::FocusAddressBar),
        // Git 面板 (GAP-32)
        "ToggleGitPanel" | "GitPanel" => Ok(Action::ToggleGitPanel),
        "ToggleLeftSidebar" | "LeftSidebar" => Ok(Action::ToggleLeftSidebar),
        "ToggleRightSidebar" | "RightSidebar" => Ok(Action::ToggleRightSidebar),
        "OpenLazygit" | "Lazygit" => Ok(Action::OpenLazygit),
        // 通知面板 (GAP-09)
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

        // 字体 / 窗口
        "IncreaseFontSize" | "FontLarger" => Ok(Action::IncreaseFontSize),
        "DecreaseFontSize" | "FontSmaller" => Ok(Action::DecreaseFontSize),
        "ResetFontSize" | "FontReset" => Ok(Action::ResetFontSize),
        "ToggleFullscreen" => Ok(Action::ToggleFullscreen),
        "SpawnNewInstance" => Ok(Action::SpawnNewInstance),

        // Vi / 复制模式
        "ToggleViMode" | "ViMode" | "CopyMode" => Ok(Action::ToggleViMode),

        // 搜索 (GAP-3.8 / GAP-4.3)
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
