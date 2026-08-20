//! GAP-19:右侧边栏——Find / Feed / Git 模式标签页。

use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme};

use crate::git_panel::GitQuickAction;
use crate::iced_app::Message;
use crate::sidebar::RightMode;
use crate::status_bar::GitOp;

/// Git 标签页的面板数据(镜像覆盖层字段)。
pub struct GitTabData<'a> {
    pub status: Option<&'a crate::status_bar::git::GitStatus>,
    pub pr: Option<&'a crate::gh_pr::PrInfo>,
    pub pr_refreshing: bool,
    pub pr_error: Option<&'a str>,
    pub last_log: &'a [(String, String)],
    pub cwd: Option<&'a str>,
    pub show_pr: bool,
}

/// Find 标签页的表面搜索字段。
pub struct SurfaceFindData<'a> {
    pub query: &'a str,
    pub case_sensitive: bool,
    pub regex_mode: bool,
    pub whole_word: bool,
    pub follow_tail: bool,
}

/// Find 标签页的目录搜索字段。
pub struct DirFindData<'a> {
    pub query: &'a str,
    pub running: bool,
    pub error: Option<&'a str>,
    pub results: &'a [crate::search::RgMatch],
}

/// 工具腰带标签页数据(GAP-30)。
pub struct ToolbeltTabData<'a> {
    pub history: &'a [String],
    pub bookmarks: &'a [crate::toolbelt::ToolbeltBookmark],
    pub snippets: &'a [crate::toolbelt::ToolbeltSnippet],
    pub paste_history: &'a [String],
}

/// Ports 标签页的端口扫描行(GAP-33)。
pub struct PortScannerTabData<'a> {
    pub entries: &'a [crate::port_scanner::ListeningPort],
    pub scanning: bool,
}

/// Files 标签页的远程目录列表(GAP-43)。
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

/// Blocks 标签页的块列表(GAP-51)。
///
/// 过滤在视图中内联完成——`exit_filter` + `search` 是
/// 参数。视图调用 `PaneBlockStore::filtered` 取得最终
/// 结果(单一事实来源),按最新优先分页并渲染
/// 页面。本结构体只承载配置/状态。
pub struct BlocksTabData<'a> {
    pub store: &'a crate::output_blocks::PaneBlockStore,
    pub exit_filter: crate::output_blocks::BlockExitFilter,
    pub search: &'a str,
    pub enabled: bool,
    pub page: usize,
    pub page_size: usize,
    pub pane_id: u64,
}
