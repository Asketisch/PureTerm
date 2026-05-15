use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::RwLock as StdRwLock;

use pureterm_core::config::AppearanceConfig;
use pureterm_core::trigger::TriggerService;
use pureterm_core::prevent_sleep::PreventSleep;
use pureterm_core::theme::{ColorScheme, ThemeService};
use pureterm_gui::EguiBackend;
use pureterm_gui::TerminalRenderer;
use pureterm_gui::renderer::ViewportRect;
use pureterm_gui::transparency::TransparencyConfig;
use pureterm_session::base::{ConnectParams, SessionType, SshAuth};
use pureterm_session::Session;
use pureterm_session::LocalSession;
use pureterm_session::SshSession;
use pureterm_session::SerialSession;
use pureterm_session::TelnetSession;
use pureterm_ui::app::App;
use pureterm_ui::command_palette::CommandPalette;
use pureterm_ui::context_menu::ContextMenuState;
use pureterm_ui::profile_selector::ProfileSelector;
use pureterm_ui::settings::SettingsPanel;
use pureterm_ui::split::{SplitContainer, Direction, PaneRect,
    compute_pane_rects};
use pureterm_ui::status_bar::StatusBar;
use pureterm_ui::tabs::TabContent;
use pureterm_ui::toolbelt::ToolbeltState;
use pureterm_ui::tab_bar::{TabBarState, TabLayout, ViewportRect as TabViewportRect, TAB_BAR_HEIGHT, TAB_ITEM_HEIGHT, TAB_PADDING_H, TAB_CLOSE_BTN_WIDTH, TAB_NEW_BTN_WIDTH};
use pureterm_ui::terminal::pane::TerminalPane;
use pureterm_ui::WelcomePage;
use pureterm_terminal::mark::MarkKind;

use winit::keyboard::ModifiersState;
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::quake_mode::QuakeMode;
use crate::platform;

/// 事件循环的应用程序状态。
pub(crate) struct PureTermApp {
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) renderer: Option<TerminalRenderer>,
    pub(crate) surface: Option<wgpu::Surface<'static>>,
    pub(crate) app: Option<App>,
    /// 面板注册表：将面板 ID 映射到 TerminalPane。
    pub(crate) panes: HashMap<u64, TerminalPane>,
    /// 活动标签页的分割容器树。
    pub(crate) split_root: Option<SplitContainer>,
    /// 当前聚焦的面板（接收键盘输入）。
    pub(crate) focused_pane_id: Option<u64>,
    /// 原生上下文菜单状态。
    pub(crate) context_menu: ContextMenuState,
    pub(crate) color_scheme: ColorScheme,
    pub(crate) modifiers: ModifiersState,
    /// 上次已知鼠标位置（像素）。
    pub(crate) mouse_pos: (f32, f32),
    /// 用于复制/粘贴的剪贴板内容。
    pub(crate) clipboard_text: String,
    /// 活动分隔符拖拽状态。
    pub(crate) divider_drag: Option<DividerDrag>,
    /// 悬停分隔符信息，用于光标高亮。
    pub(crate) hovered_divider: Option<pureterm_ui::split::DividerInfo>,
    /// 用于设置覆盖层的 egui 后端。
    pub(crate) egui_backend: Option<EguiBackend>,
    /// 设置面板状态。
    pub(crate) settings_panel: SettingsPanel,
    /// 命令面板状态。
    pub(crate) command_palette: CommandPalette,
    /// 用于快速连接/会话选择的配置文件选择器。
    pub(crate) profile_selector: ProfileSelector,
    /// 首次启动或没有标签页时的欢迎页面。
    pub(crate) welcome_page: WelcomePage,
    /// 标签页标题重命名模态框状态。
    pub(crate) rename_modal: Option<RenameModalState>,
    /// 标签页颜色编辑模态框状态。
    pub(crate) color_modal: Option<ColorModalState>,
    /// Quake 模式（下拉终端）状态。
    pub(crate) quake_mode: QuakeMode,
    /// 标签栏状态。
    pub(crate) tab_bar: TabBarState,
    /// 运行时计算的红绿灯区域宽度（物理像素）。
    pub(crate) traffic_light_w: f32,
    /// 用于处理调整大小的外观配置。
    pub(crate) appearance: AppearanceConfig,
    /// 用于从后台线程唤醒事件循环的事件循环代理。
    pub(crate) proxy: EventLoopProxy<()>,
    /// Shared buffer for OSC 133 mark events from terminal panes.
    pub(crate) pending_marks: Arc<Mutex<Vec<(u64, MarkKind)>>>,
    /// Shared trigger rules; injected into every new TerminalPane.
    pub(crate) trigger_service: Arc<StdRwLock<TriggerService>>,
    /// Shared auto-profile switcher; injected into every new TerminalPane.
    pub(crate) auto_profile_switcher: Arc<StdRwLock<pureterm_core::auto_profile::AutoProfileSwitcher>>,
    /// Shared smart-selection service; injected into every new TerminalPane.
    pub(crate) smart_selection: Arc<pureterm_core::smart_selection::SmartSelectionService>,
    /// 持久化的主题服务，加载了用户主题
    pub(crate) theme_service: ThemeService,
    /// Prevent system sleep while commands are running.
    pub(crate) prevent_sleep: PreventSleep,
    /// Whether sleep is currently being prevented (for UI indicator).
    pub(crate) sleep_prevented: bool,
    /// Whether a close was requested and we need to show the quit confirmation dialog.
    pub(crate) quit_requested: bool,
    /// Whether the application should actually exit now.
    pub(crate) should_quit: bool,
    /// Last left-click time on a tab (for double-click detection to trigger rename).
    pub(crate) last_tab_click_time: Option<std::time::Instant>,
    /// Last left-clicked tab ID (for double-click detection).
    pub(crate) last_tab_click_id: Option<u64>,
    /// Bottom status bar (cwd / git branch / clock by default).
    pub(crate) status_bar: StatusBar,
    /// Right-side Toolbelt panel state (visibility gated by config).
    pub(crate) toolbelt: ToolbeltState,
}

/// 标签页标题重命名模态框的状态。
pub(crate) struct RenameModalState {
    pub(crate) tab_id: u64,
    pub(crate) new_title: String,
}

/// 标签页颜色编辑模态框的状态。
pub(crate) struct ColorModalState {
    pub(crate) tab_id: u64,
    pub(crate) color: String,
}

/// 活动分隔符拖拽的状态。
pub(crate) struct DividerDrag {
    /// 拥有被拖拽分隔符的容器 ID。
    pub(crate) container_id: u64,
    /// 左侧/顶部子节点的索引，其右/下边缘是分隔符。
    pub(crate) child_index: usize,
    /// 分隔符是垂直（左右）还是水平（上下）方向。
    pub(crate) direction: Direction,
    /// 拖拽开始的像素位置。
    pub(crate) start_pos: (f32, f32),
    /// 容器级别在分割方向上的可用总大小。
    pub(crate) total_size: f32,
    /// 被拖拽分隔符的可见矩形（用于高亮匹配）。
    pub(crate) divider_rect: PaneRect,
}

impl PureTermApp {
    /// 获取聚焦面板的引用。
    pub(crate) fn focused_pane(&self) -> Option<&TerminalPane> {
        self.focused_pane_id.and_then(|id| self.panes.get(&id))
    }

    /// 获取聚焦面板的可变引用。
    pub(crate) fn focused_pane_mut(&mut self) -> Option<&mut TerminalPane> {
        self.focused_pane_id.and_then(move |id| self.panes.get_mut(&id))
    }

    /// 显示上下文菜单并根据当前字体测量其宽度。
    pub(crate) fn show_context_menu(&mut self, x: f32, y: f32, pane_id: u64) {
        let char_width = self.renderer.as_ref()
            .map(|r| r.cell_size().0)
            .unwrap_or(8.0);
        self.context_menu.show_at(x, y, pane_id);
        self.context_menu.measure_width(char_width);
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    /// 计算当前布局（面板矩形）。
    pub(crate) fn compute_layout(&self, screen_w: f32, screen_h: f32) -> Vec<(u64, PaneRect)> {
        let tab_bar_offset_y = if self.show_tab_bar() { self.tab_bar.height } else { 0.0 };
        if let Some(ref root) = self.split_root {
            compute_pane_rects(root, 0.0, tab_bar_offset_y, screen_w, screen_h - tab_bar_offset_y)
        } else {
            // 单个面板填充整个窗口
            let pane_id = match self.focused_pane_id {
                Some(id) => id,
                None => return Vec::new(),
            };
            vec![(pane_id, PaneRect { x: 0.0, y: tab_bar_offset_y, w: screen_w, h: screen_h - tab_bar_offset_y })]
        }
    }

    /// 检查是否应显示标签栏。
    pub(crate) fn show_tab_bar(&self) -> bool {
        let config = self.app.as_ref().map(|a| &a.config.appearance);
        let show = config.map(|c| c.show_tab_bar).unwrap_or(true);
        let location = config.map(|c| c.tabs_location.as_str()).unwrap_or("top");
        show && location == "top"
    }

    /// 渲染前更新标签栏布局。
    pub(crate) fn update_tab_bar_layout(&mut self) {
        let (screen_w, _screen_h) = self.screen_size();
        let show = self.show_tab_bar();
        let scale = self.window.as_ref().map(|w| w.scale_factor() as f32).unwrap_or(1.0);

        self.tab_bar.visible = show;

        if !show {
            self.tab_bar.tab_layouts.clear();
            self.tab_bar.new_tab_rect = TabViewportRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
            return;
        }

        // 缓存 scaled 常量到 TabBarState，供渲染和布局使用
        let bar_h = TAB_BAR_HEIGHT * scale;
        let item_h = TAB_ITEM_HEIGHT * scale;
        let item_y = (bar_h - item_h) / 2.0;
        let new_btn_w = TAB_NEW_BTN_WIDTH * scale;
        let close_btn_w = TAB_CLOSE_BTN_WIDTH * scale;
        let padding_h = TAB_PADDING_H * scale;

        self.tab_bar.scale = scale;
        self.tab_bar.height = bar_h;
        self.tab_bar.bar_h = bar_h;
        self.tab_bar.item_h = item_h;
        self.tab_bar.item_y = item_y;
        self.tab_bar.close_btn_w = close_btn_w;
        self.tab_bar.new_btn_w = new_btn_w;
        self.tab_bar.padding_h = padding_h;

        let tabs = self.app.as_ref().map(|a| a.tab_manager.tabs()).unwrap_or(&[]);
        self.tab_bar.tab_layouts.clear();

        // "+" 新标签按钮位于标签栏右端，高度与标签项一致
        let new_tab_x = screen_w - new_btn_w;
        self.tab_bar.new_tab_rect = TabViewportRect {
            x: new_tab_x,
            y: item_y,
            w: new_btn_w,
            h: item_h,
        };

        let tab_count = tabs.len();
        if tab_count == 0 {
            return;
        }

        // 所有标签均分：终端宽度 - 红绿灯背景 - 新建标签按钮
        let tl_w = self.traffic_light_w;
        let tabs_available_w = new_tab_x - tl_w;
        let equal_width = tabs_available_w / tab_count as f32;

        let mut x = tl_w;
        for tab in tabs {
            let cw = if tab_count > 1 { close_btn_w } else { 0.0 };

            let rect = TabViewportRect {
                x,
                y: item_y,
                w: equal_width,
                h: item_h,
            };

            let close_rect = if cw > 0.0 {
                Some(TabViewportRect {
                    x: x + 4.0 * scale,
                    y: item_y,
                    w: cw,
                    h: item_h,
                })
            } else {
                None
            };

            self.tab_bar.tab_layouts.push(TabLayout {
                tab_id: tab.id,
                title: tab.title.clone(),
                color: tab.color.clone(),
                rect,
                close_button_rect: close_rect,
            });

            x += equal_width;
        }
    }

    /// 查找给定像素位置处的面板。
    #[allow(dead_code)]
    pub(crate) fn pane_at_point(&self, px: f32, py: f32, screen_w: f32, screen_h: f32) -> Option<u64> {
        let layout = self.compute_layout(screen_w, screen_h);
        pureterm_ui::split::pane_at_point(&layout, px, py)
    }

    /// 将像素位置转换为特定面板的单元格坐标。
    pub(crate) fn pixel_to_cell_for_pane(&self, pane_id: u64, px: f32, py: f32, layout: &[(u64, PaneRect)]) -> (usize, usize) {
        let renderer = match &self.renderer {
            Some(r) => r,
            None => return (0, 0),
        };
        let (cell_w, cell_h) = renderer.cell_size();

        // 查找面板矩形以计算局部坐标
        let rect = layout.iter().find(|(id, _)| *id == pane_id).map(|(_, r)| r);
        let (local_x, local_y) = match rect {
            Some(r) => (px - r.x, py - r.y),
            None => (px, py),
        };

        let col = (local_x / cell_w).floor() as usize;
        let row = (local_y / cell_h).floor() as usize;
        (row.max(0), col.max(0))
    }

    /// 获取屏幕尺寸。
    pub(crate) fn screen_size(&self) -> (f32, f32) {
        self.renderer.as_ref()
            .and_then(|r| r.surface_config())
            .map(|c| (c.width as f32, c.height as f32))
            .unwrap_or((0.0, 0.0))
    }

    /// 调整所有面板大小以适应其分配的空间。
    pub(crate) fn resize_all_panes(&mut self) {
        let (screen_w, screen_h) = self.screen_size();
        if screen_w <= 0.0 || screen_h <= 0.0 {
            return;
        }

        let renderer = match &self.renderer {
            Some(r) => r,
            None => return,
        };
        let (cell_w, cell_h) = renderer.cell_size();
        let layout = self.compute_layout(screen_w, screen_h);

        for (pane_id, rect) in &layout {
            if let Some(pane) = self.panes.get_mut(pane_id) {
                let cols = ((rect.w / cell_w).floor() as usize).max(1);
                let rows = ((rect.h / cell_h).floor() as usize).max(1);
                pane.resize(cols, rows);
            }
        }
    }
}
