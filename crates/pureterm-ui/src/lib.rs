pub mod app;
pub mod ai_chat;
pub mod autocomplete;
pub mod clipboard;
pub mod command_palette;
pub mod context_menu;
pub mod explorer;
pub mod focus_mode;
pub mod jump_prompt;
pub mod lock_screen;
pub mod online_search;
pub mod quick_bar;
pub mod search;
pub mod settings;
pub mod split;
pub mod tabs;
pub mod tab_recovery;
pub mod terminal;

pub use app::App;
pub use clipboard::ClipboardManager;
pub use command_palette::CommandPalette;
pub use context_menu::{ContextMenu, ContextMenuState, MenuAction, MenuItem,
    MENU_ITEM_HEIGHT, MENU_MIN_WIDTH, MENU_PADDING, MENU_SEPARATOR_HEIGHT};
pub use settings::SettingsPanel;
pub use split::{SplitContainer, PaneRect, compute_pane_rects, pane_at_point, compute_divider_rects};
pub use tabs::TabManager;
pub use tab_recovery::{TabRecovery, SavedTab, SavedWindowState};
pub use terminal::TerminalPane;
