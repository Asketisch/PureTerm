//! GAP-19: Right sidebar — Find / Feed / Git mode tabs.

use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme};

use crate::git_panel::GitQuickAction;
use crate::iced_app::Message;
use crate::sidebar::RightMode;
use crate::status_bar::GitOp;

/// Git panel data for the Git tab (mirrors overlay fields).
pub struct GitTabData<'a> {
    pub status: Option<&'a crate::status_bar::git::GitStatus>,
    pub pr: Option<&'a crate::gh_pr::PrInfo>,
    pub pr_refreshing: bool,
    pub pr_error: Option<&'a str>,
    pub last_log: &'a [(String, String)],
    pub cwd: Option<&'a str>,
    pub show_pr: bool,
}

/// Surface search fields for the Find tab.
pub struct SurfaceFindData<'a> {
    pub query: &'a str,
    pub case_sensitive: bool,
    pub regex_mode: bool,
    pub whole_word: bool,
    pub follow_tail: bool,
}

/// Directory search fields for the Find tab.
pub struct DirFindData<'a> {
    pub query: &'a str,
    pub running: bool,
    pub error: Option<&'a str>,
    pub results: &'a [crate::search::RgMatch],
}

/// Toolbelt tab data (GAP-30).
pub struct ToolbeltTabData<'a> {
    pub history: &'a [String],
    pub bookmarks: &'a [crate::toolbelt::ToolbeltBookmark],
    pub snippets: &'a [crate::toolbelt::ToolbeltSnippet],
    pub paste_history: &'a [String],
}

/// Port scanner rows for the Ports tab (GAP-33).
pub struct PortScannerTabData<'a> {
    pub entries: &'a [crate::port_scanner::ListeningPort],
    pub scanning: bool,
}

/// Remote directory listing for the Files tab (GAP-43).
pub struct RemoteFilesTabData<'a> {
    pub path: &'a str,
    pub entries: &'a [crate::daemon_client::RemoteFileEntry],
    pub loading: bool,
    pub error: Option<&'a str>,
    pub session_label: Option<&'a str>,
    pub can_go_up: bool,
    pub preview_path: Option<&'a str>,
    pub preview_loading: bool,
    pub preview: Option<&'a crate::daemon_client::RemoteFileRead>,
    pub preview_error: Option<&'a str>,
    pub find_query: &'a str,
    pub find_results: &'a [crate::daemon_client::RemoteFindMatch],
    pub find_loading: bool,
    pub find_error: Option<&'a str>,
}

/// Block list for the Blocks tab (GAP-51).
pub struct BlocksTabData<'a> {
    pub blocks: &'a [crate::output_blocks::OutputBlock],
    pub exit_filter: crate::output_blocks::BlockExitFilter,
    pub search: &'a str,
    pub enabled: bool,
}
