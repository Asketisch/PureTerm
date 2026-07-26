//! 默认键绑定表 — `Keymap::from_defaults` 注册的所有 macOS 默认快捷键。
//!
//! 这是一张纯数据表（`(Modifiers, BindingKey) → Action` 映射），独立成文件
//! 便于查阅「出厂」绑定全貌。绑定逻辑在 `keymap.rs`。

use std::collections::HashMap;

use iced::keyboard::Modifiers;

use super::action::Action;
use super::types::{BindingKey, BindingNamedKey};
use crate::split_tree::SplitDirection;

/// Build the hardcoded default single-stroke binding table.
///
/// Exactly reproduces the pre-keymap `handle_keyboard` inline match behaviour.
/// macOS reports ⌘⇧<letter> inconsistently (sometimes as lowercase+shift,
/// sometimes as uppercase without shift) — dual-form keys register both forms.
pub fn default_bindings() -> HashMap<(Modifiers, BindingKey), Action> {
    let mut bindings = HashMap::new();

    // macOS reports ⌘ as `command()`.
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    let cmd_alt = Modifiers::COMMAND | Modifiers::ALT;
    let ctrl = Modifiers::CTRL;
    let cmd_ctrl = Modifiers::COMMAND | Modifiers::CTRL;

    // ── Tab management ──────────────────────────────────────────
    bindings.insert((cmd, BindingKey::Character("t".into())), Action::NewTab);
    // ⌘⇧T / ⌘T-with-uppercase-T → MovePaneToNewTab
    // macOS may report ⌘⇧T as Character("T") with shift NOT set,
    // so we register both forms.
    bindings.insert((cmd, BindingKey::Character("T".into())), Action::MovePaneToNewTab);
    bindings.insert((cmd_shift, BindingKey::Character("t".into())), Action::MovePaneToNewTab);
    // ⌘W → ClosePaneOrTab
    bindings.insert((cmd, BindingKey::Character("w".into())), Action::ClosePaneOrTab);
    // ⌘} → NextTab (macOS reports ⌘⇧] as "}")
    bindings.insert((cmd, BindingKey::Character("}".into())), Action::NextTab);
    // ⌘{ → PrevTab (macOS reports ⌘⇧[ as "{")
    bindings.insert((cmd, BindingKey::Character("{".into())), Action::PrevTab);
    // ⌘] → FocusPaneInOrder(1)
    bindings.insert((cmd, BindingKey::Character("]".into())), Action::FocusPaneInOrder(1));
    // ⌘[ → FocusPaneInOrder(-1)
    bindings.insert((cmd, BindingKey::Character("[".into())), Action::FocusPaneInOrder(-1));
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
    bindings.insert((cmd, BindingKey::Character("d".into())), Action::SplitHorizontal);
    // ⌘⇧D → SplitVertical (macOS may report as "D" without shift)
    bindings.insert((cmd_shift, BindingKey::Character("d".into())), Action::SplitVertical);
    bindings.insert((cmd, BindingKey::Character("D".into())), Action::SplitVertical);
    // GAP-3.5 SplitWeb: no default ⌘⇧L binding — GAP-01 AI Chat uses ⌘⇧L.
    // SplitWeb remains available via command palette / `[[keyboard.bindings]]`.
    // ⌘= → EqualizeSplits
    bindings.insert((cmd, BindingKey::Character("=".into())), Action::EqualizeSplits);
    // ⌘⇧X → SwapPane
    bindings.insert((cmd_shift, BindingKey::Character("x".into())), Action::SwapPane);
    bindings.insert((cmd, BindingKey::Character("X".into())), Action::SwapPane);
    // ⌘⇧Enter → ToggleZoom
    bindings.insert(
        (cmd_shift, BindingKey::Named(BindingNamedKey::Enter)),
        Action::ToggleZoom,
    );
    // ⌘⇧N → NewWindow (GAP-25); MovePaneToNewWindow stays on context menu.
    bindings.insert((cmd_shift, BindingKey::Character("n".into())), Action::NewWindow);
    bindings.insert((cmd, BindingKey::Character("N".into())), Action::NewWindow);
    // ⌘⇧R → Instant Replay (GAP-11)
    bindings.insert((cmd_shift, BindingKey::Character("r".into())), Action::ToggleInstantReplay);
    bindings.insert((cmd, BindingKey::Character("R".into())), Action::ToggleInstantReplay);
    // ⌘⇧↑/↓ → mark navigation (GAP-12)
    bindings.insert(
        (cmd_shift, BindingKey::Named(BindingNamedKey::ArrowUp)),
        Action::PrevMark,
    );
    bindings.insert(
        (cmd_shift, BindingKey::Named(BindingNamedKey::ArrowDown)),
        Action::NextMark,
    );

    // ── Clipboard ───────────────────────────────────────────────
    bindings.insert((cmd, BindingKey::Character("v".into())), Action::ClipboardPaste);
    bindings.insert((cmd, BindingKey::Character("c".into())), Action::ClipboardCopy);
    bindings.insert((cmd, BindingKey::Character("a".into())), Action::SelectAll);

    // ── Dialogs (platform-gated at dispatch level) ──────────────
    // ⌘⇧S → OpenSshDialog (macOS reports as "S" without shift)
    bindings.insert((cmd_shift, BindingKey::Character("s".into())), Action::OpenSshDialog);
    bindings.insert((cmd, BindingKey::Character("S".into())), Action::OpenSshDialog);
    // Also match plain ⌘s for safety (macOS dual-form)
    bindings.insert((cmd, BindingKey::Character("s".into())), Action::OpenSshDialog);
    // ⌘⇧P → OpenCommandPalette (macOS reports as "P" without shift)
    bindings.insert((cmd_shift, BindingKey::Character("p".into())), Action::OpenCommandPalette);
    bindings.insert((cmd, BindingKey::Character("P".into())), Action::OpenCommandPalette);
    bindings.insert((cmd, BindingKey::Character("p".into())), Action::OpenCommandPalette);

    // ── Browser (GAP-3.5) ──────────────────────────────────────
    // ⌘L → FocusAddressBar (navigate the active Web pane / spawn one).
    bindings.insert((cmd, BindingKey::Character("l".into())), Action::FocusAddressBar);

    // ── Sidebar (GAP-19) ─────────────────────────────────────────
    // ⌘B → left sidebar (Workspace + Git summary).
    bindings.insert((cmd, BindingKey::Character("b".into())), Action::ToggleLeftSidebar);
    // ⌘⇧B → right sidebar (Find / Feed / Git). macOS dual-form.
    bindings.insert((cmd_shift, BindingKey::Character("b".into())), Action::ToggleRightSidebar);
    bindings.insert((cmd, BindingKey::Character("B".into())), Action::ToggleRightSidebar);

    // ── GAP-13 Broadcast ─────────────────────────────────────────
    let cmd_ctrl = cmd | Modifiers::CTRL;
    bindings.insert(
        (cmd_ctrl, BindingKey::Character("b".into())),
        Action::ToggleBroadcastInput,
    );

    // ── GAP-31 Quake dock ────────────────────────────────────────
    let ctrl = Modifiers::CTRL;
    bindings.insert(
        (ctrl, BindingKey::Named(BindingNamedKey::F(12))), // MVP: F12 when ` not bindable
        Action::ToggleQuakeDock,
    );

    // ── GAP-30 Toolbelt ──────────────────────────────────────────
    bindings.insert((cmd_shift, BindingKey::Character("t".into())), Action::ToggleToolbelt);

    // Legacy Git panel alias → right sidebar Git tab.
    // (Command palette / custom bindings may still say ToggleGitPanel.)

    // ── Notifications panel (GAP-09) ───────────────────────────
    // ⌘⇧M → ToggleNotificationsPanel ("Messages"). macOS dual-form
    // (⌘⇧M may report as ⌘M-uppercase), mirroring the ⌘⇧B git-panel trick.
    // Plain ⌘M/⌘H/⌘Q are left to macOS (minimize/hide/quit).
    bindings.insert((cmd_shift, BindingKey::Character("m".into())), Action::ToggleNotificationsPanel);
    bindings.insert((cmd, BindingKey::Character("M".into())), Action::ToggleNotificationsPanel);

    // ── Error Recovery (GAP-02) ───────────────────────────────────
    // ⌘⇧E → toggle / re-analyze failed command. macOS dual-form.
    bindings.insert((cmd_shift, BindingKey::Character("e".into())), Action::ErrorRecoveryToggle);
    bindings.insert((cmd, BindingKey::Character("E".into())), Action::ErrorRecoveryToggle);

    // ── AI Chat Panel (GAP-01) ────────────────────────────────────
    // ⌘⇧L → ToggleAiChatPanel. macOS dual-form. Plain ⌘L stays on
    // FocusAddressBar (Browser) — do not bind chat to ⌘L.
    bindings.insert((cmd_shift, BindingKey::Character("l".into())), Action::ToggleAiChatPanel);
    bindings.insert((cmd, BindingKey::Character("L".into())), Action::ToggleAiChatPanel);

    // ── Hover Preview (GAP-4.2) ───────────────────────────────────
    // ⌘⇧O → open hovered/selected URL or path. macOS dual-form.
    bindings.insert((cmd_shift, BindingKey::Character("o".into())), Action::OpenHoverTarget);
    bindings.insert((cmd, BindingKey::Character("O".into())), Action::OpenHoverTarget);

    // ── File Preview (GAP-34) ────────────────────────────────────
    // ⌘Y → QuickLook preview of hovered/selected file.
    bindings.insert((cmd, BindingKey::Character("y".into())), Action::PreviewFile);

    // ── Cmd+Option+Arrow → PaneFocusAdjacent ────────────────────
    // Ctrl+Shift+Space → ToggleViMode (copy mode)
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    bindings.insert(
        (ctrl_shift, BindingKey::Named(BindingNamedKey::Space)),
        Action::ToggleViMode,
    );

    // ── Search (GAP-3.8 / GAP-4.3) ──────────────────────────────
    // ⌘F → open current-surface search overlay.
    bindings.insert((cmd, BindingKey::Character("f".into())), Action::SearchForward);
    // ⌘⇧F → working-directory (ripgrep) search. macOS dual-form.
    bindings.insert((cmd_shift, BindingKey::Character("f".into())), Action::SearchInDirectory);
    bindings.insert((cmd, BindingKey::Character("F".into())), Action::SearchInDirectory);
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
    bindings.insert((cmd, BindingKey::Character("g".into())), Action::SearchNext);
    bindings.insert((cmd_shift, BindingKey::Character("g".into())), Action::SearchPrev);

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

    bindings
}
