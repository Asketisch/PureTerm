pub mod app;
pub mod ai_chat;
pub mod autocomplete;
pub mod clipboard;
pub mod command_palette;
pub mod context_menu;
pub mod explorer;
pub mod focus_mode;
pub mod history_menu;
pub mod inspector;
pub mod jump_prompt;
pub mod lock_screen;
pub mod online_search;
pub mod profile_selector;
pub mod quick_bar;
pub mod search;
pub mod settings;
pub mod split;
pub mod split_tree;
pub mod surface;
pub mod tabs;
pub mod tab_recovery;
pub mod tab_bar;
pub mod terminal;
pub mod theme;
pub mod tokens;
pub mod welcome;

pub use app::App;
pub use clipboard::ClipboardManager;
pub use command_palette::CommandPalette;
pub use context_menu::{ContextMenu, ContextMenuState, MenuAction,
    MENU_ITEM_HEIGHT, MENU_MIN_WIDTH, MENU_PADDING_H, MENU_PADDING_V, MENU_SEPARATOR_HEIGHT};
pub use inspector::{
    InspectorState, TerminalDebugInfo, RendererDebugInfo,
    GridDebugInfo, ModesDebugInfo, FontAtlasDebugInfo, PerfDebugInfo, ConfigDebugInfo,
};
pub use profile_selector::ProfileSelector;
pub use settings::SettingsPanel;
pub use split::{SplitContainer, PaneRect, SplitSide, DividerInfo, compute_pane_rects, pane_at_point, compute_divider_rects, divider_at_point, DIVIDER_WIDTH, DIVIDER_HIT_WIDTH};
pub use surface::{Surface, SurfaceId, SurfaceSize, SurfaceMouse, KeyboardState,
    Action, ActionTarget, CursorShape, SplitDirection, SplitResizeParameter};
pub use tabs::TabManager;
pub use tab_recovery::{TabRecovery, SavedTab, SavedWindowState};
pub use terminal::TerminalPane;
pub use theme::{ColorTheme, apply_theme, set_theme, dark_mode, light_mode, dark_color_theme, light_color_theme};
pub use welcome::WelcomePage;
