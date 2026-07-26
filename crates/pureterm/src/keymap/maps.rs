//! 命令面板 / 右键菜单 → `Action` 的映射。
//!
//! palette builtin id 与 `ContextMenuItem` 各自映射到 `Action`；返回 `None`
//! 表示该项需要 tab_id 上下文或走内联分派（如 "settings"）。

use super::action::Action;

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
