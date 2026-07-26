//! `Action` 枚举 — 所有用户可触发操作的统一词汇表。
//!
//! 取代 `handle_keyboard()` 里三条并行分派路径（keyboard match / palette
//! string match / context menu match）的内联 match。

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
    /// GAP-25: open a new independent window (⌘⇧N).
    NewWindow,

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
    PaneFocusAdjacent(crate::split_tree::SplitDirection),

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
    /// GAP-32: open Git tab in the right sidebar (legacy alias; ⌘⇧B is ToggleRightSidebar).
    ToggleGitPanel,
    /// GAP-19: toggle the left sidebar (⌘B) — Workspace list + Git summary.
    ToggleLeftSidebar,
    /// GAP-19: toggle the right sidebar (⌘⇧B) — Find / Feed / Git tabs.
    ToggleRightSidebar,
    /// GAP-19: switch the active right-sidebar tab.
    SidebarRightMode(crate::sidebar::RightMode),
    /// GAP-32: open lazygit in the active pane (panel button / command palette).
    OpenLazygit,
    /// GAP-09: toggle the Notifications history overlay panel (⌘⇧M).
    ToggleNotificationsPanel,
    /// GAP-02: toggle / re-analyze the error-recovery card (⌘⇧E).
    ErrorRecoveryToggle,
    /// GAP-01: toggle the AI Chat panel (⌘⇧L — ⌘L is Browser address bar).
    ToggleAiChatPanel,
    /// GAP-4.2: open hovered / selected URL or file path (⌘⇧O).
    OpenHoverTarget,
    /// GAP-36: open hovered / selected file path in editor (semantic history).
    OpenSemanticInEditor,
    /// GAP-34: preview hovered / selected file (Cmd+Y QuickLook).
    PreviewFile,

    /// GAP-11: toggle instant replay overlay (⌘⇧R).
    ToggleInstantReplay,
    /// GAP-11: scrub replay timeline (←/→ in overlay).
    ReplayScrub(i32),
    /// GAP-12: jump to previous shell mark (⌘⇧↑).
    PrevMark,
    /// GAP-12: jump to next shell mark (⌘⇧↓).
    NextMark,

    /// GAP-13: toggle broadcast input (⌘⌃B).
    ToggleBroadcastInput,
    /// GAP-31: toggle quake-style dock (⌃`).
    ToggleQuakeDock,
    /// GAP-30: open Toolbelt tab in right sidebar.
    ToggleToolbelt,

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
