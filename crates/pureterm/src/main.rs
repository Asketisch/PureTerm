use anyhow::Result;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

mod quake_mode;
mod platform;

use pureterm_core::config::ConfigService;
use pureterm_core::profile::{Profile, ProfileOptions, ProfileType};
use pureterm_core::theme::ThemeService;
use pureterm_gui::TerminalRenderer;
use pureterm_gui::EguiBackend;
use pureterm_gui::renderer::ViewportRect;
use pureterm_gui::transparency::TransparencyConfig;
use pureterm_session::base::{ConnectParams, SessionType, SshAuth};
use pureterm_session::Session;
use pureterm_session::LocalSession;
use pureterm_session::SshSession;
use pureterm_session::SerialSession;
use pureterm_session::TelnetSession;
use pureterm_ui::app::App;
use pureterm_ui::context_menu::{ContextMenuState, MenuAction,
    MENU_ITEM_HEIGHT, MENU_SEPARATOR_HEIGHT};
use pureterm_ui::profile_selector::ProfileSelector;
use pureterm_ui::settings::SettingsPanel;
use pureterm_ui::apply_theme;
use quake_mode::QuakeMode;
use pureterm_ui::split::{SplitContainer, Direction, SplitSide, PaneRect, DividerInfo,
    compute_pane_rects, pane_at_point, compute_divider_rects, divider_at_point};
use pureterm_ui::tabs::{TabContent, SplitRef};
use pureterm_ui::tab_bar::{TabBarState, TabLayout, ViewportRect as TabViewportRect, TAB_BAR_HEIGHT, TAB_MIN_WIDTH, TAB_PADDING_H, TAB_CLOSE_BTN_WIDTH, TAB_NEW_BTN_WIDTH, TRAFFIC_LIGHT_WIDTH};
use pureterm_ui::terminal::pane::{TerminalPane, PaneAction, Selection};
use pureterm_ui::WelcomePage;
use pureterm_terminal::screen::selection::{TerminalMouseEvent, MouseButton, MouseModifiers};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent, MouseButton as WinitMouseButton, MouseScrollDelta};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::Window;

/// 事件循环的应用程序状态。
struct PureTermApp {
    window: Option<Arc<Window>>,
    renderer: Option<TerminalRenderer>,
    surface: Option<wgpu::Surface<'static>>,
    app: Option<App>,
    /// 面板注册表：将面板 ID 映射到 TerminalPane。
    panes: HashMap<u64, TerminalPane>,
    /// 活动标签页的分割容器树。
    split_root: Option<SplitContainer>,
    /// 当前聚焦的面板（接收键盘输入）。
    focused_pane_id: Option<u64>,
    /// 原生上下文菜单状态。
    context_menu: ContextMenuState,
    color_scheme: pureterm_core::theme::ColorScheme,
    modifiers: ModifiersState,
    /// 上次已知鼠标位置（像素）。
    mouse_pos: (f32, f32),
    /// 用于复制/粘贴的剪贴板内容。
    clipboard_text: String,
    /// 活动分隔符拖拽状态。
    divider_drag: Option<DividerDrag>,
    /// 悬停分隔符信息，用于光标高亮。
    hovered_divider: Option<DividerInfo>,
    /// 用于设置覆盖层的 egui 后端。
    egui_backend: Option<EguiBackend>,
    /// 设置面板状态。
    settings_panel: SettingsPanel,
    /// 用于快速连接/会话选择的配置文件选择器。
    profile_selector: ProfileSelector,
    /// 首次启动或没有标签页时的欢迎页面。
    welcome_page: WelcomePage,
    /// 标签页标题重命名模态框状态。
    rename_modal: Option<RenameModalState>,
    /// 标签页颜色编辑模态框状态。
    color_modal: Option<ColorModalState>,
    /// Quake 模式（下拉终端）状态。
    quake_mode: QuakeMode,
    /// 标签栏状态。
    tab_bar: TabBarState,
    /// 用于处理调整大小的外观配置。
    appearance: pureterm_core::config::AppearanceConfig,
    /// 用于从后台线程唤醒事件循环的事件循环代理。
    proxy: winit::event_loop::EventLoopProxy<()>,
}

/// 标签页标题重命名模态框的状态。
struct RenameModalState {
    tab_id: u64,
    new_title: String,
}

/// 标签页颜色编辑模态框的状态。
struct ColorModalState {
    tab_id: u64,
    color: String,
}

/// 活动分隔符拖拽的状态。
struct DividerDrag {
    /// 拥有被拖拽分隔符的容器 ID。
    container_id: u64,
    /// 左侧/顶部子节点的索引，其右/下边缘是分隔符。
    child_index: usize,
    /// 分隔符是垂直（左右）还是水平（上下）方向。
    direction: Direction,
    /// 拖拽开始的像素位置。
    start_pos: (f32, f32),
    /// 容器级别在分割方向上的可用总大小。
    total_size: f32,
    /// 被拖拽分隔符的可见矩形（用于高亮匹配）。
    divider_rect: PaneRect,
}

impl PureTermApp {
    /// 获取聚焦面板的引用。
    fn focused_pane(&self) -> Option<&TerminalPane> {
        self.focused_pane_id.and_then(|id| self.panes.get(&id))
    }

    /// 获取聚焦面板的可变引用。
    fn focused_pane_mut(&mut self) -> Option<&mut TerminalPane> {
        self.focused_pane_id.and_then(move |id| self.panes.get_mut(&id))
    }

    /// 显示上下文菜单并根据当前字体测量其宽度。
    fn show_context_menu(&mut self, x: f32, y: f32, pane_id: u64) {
        let char_width = self.renderer.as_ref()
            .map(|r| r.cell_size().0)
            .unwrap_or(8.0);
        self.context_menu.show_at(x, y, pane_id);
        self.context_menu.measure_width(char_width);
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    /// 创建带有本地会话的新终端面板（便捷方法）。
    fn create_pane(&mut self, cols: usize, rows: usize) -> u64 {
        self.create_pane_with_session(cols, rows, ConnectParams {
            session_type: SessionType::Local { shell: None, args: Vec::new() },
            env: std::collections::HashMap::new(),
            cwd: None,
        })
    }

    /// 使用给定会话类型创建新的终端面板。
    fn create_pane_with_session(&mut self, cols: usize, rows: usize, params: ConnectParams) -> u64 {
        let mut session: Box<dyn Session> = match &params.session_type {
            SessionType::Local { .. } => {
                Box::new(LocalSession::new(params.clone()).expect("Failed to create local session"))
            }
            SessionType::Ssh { host, port, user, auth, agent_forward, .. } => {
                let ssh_auth = match auth {
                    SshAuth::None => pureterm_session::ssh::auth::SshAuth::None,
                    SshAuth::Password(pwd) => pureterm_session::ssh::auth::SshAuth::Password(pwd.clone()),
                    SshAuth::PublicKey { key_path, passphrase } => {
                        pureterm_session::ssh::auth::SshAuth::PublicKey {
                            key_path: key_path.clone(),
                            passphrase: passphrase.clone(),
                        }
                    }
                    SshAuth::Agent => pureterm_session::ssh::auth::SshAuth::Agent,
                    SshAuth::PromptPassword => pureterm_session::ssh::auth::SshAuth::PromptPassword,
                };
                Box::new(SshSession::new(host.clone(), *port, user.clone(), ssh_auth, *agent_forward))
            }
            SessionType::Serial { path, baud_rate, .. } => {
                Box::new(SerialSession::new(path.clone(), *baud_rate))
            }
            SessionType::Telnet { host, port } => {
                Box::new(TelnetSession::new(host.clone(), *port))
            }
        };

        let output_receiver = session.output_stream();
        pollster::block_on(session.start()).unwrap_or_else(|e| {
            log::error!("Failed to start session: {}", e);
        });
        session.resize(cols as u16, rows as u16).ok();

        let mut pane = TerminalPane::new(cols, rows, 25000);
        let pane_id = pane.id;
        let proxy = self.proxy.clone();
        let waker: Arc<dyn Fn() + Send + Sync> = Arc::new(move || { let _ = proxy.send_event(()); });
        pane.set_session_with_receiver(session, output_receiver, Some(waker));
        self.panes.insert(pane_id, pane);
        pane_id
    }

    /// 计算当前布局（面板矩形）。
    fn compute_layout(&self, screen_w: f32, screen_h: f32) -> Vec<(u64, PaneRect)> {
        let tab_bar_offset_y = if self.show_tab_bar() { TAB_BAR_HEIGHT } else { 0.0 };
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
    fn show_tab_bar(&self) -> bool {
        let config = self.app.as_ref().map(|a| &a.config.appearance);
        let show = config.map(|c| c.show_tab_bar).unwrap_or(true);
        let location = config.map(|c| c.tabs_location.as_str()).unwrap_or("top");
        show && location == "top"
    }

    /// 渲染前更新标签栏布局。
    fn update_tab_bar_layout(&mut self) {
        let (screen_w, _screen_h) = self.screen_size();
        let show = self.show_tab_bar();

        self.tab_bar.visible = show;

        if !show {
            self.tab_bar.tab_layouts.clear();
            self.tab_bar.new_tab_rect = TabViewportRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
            return;
        }

        let tabs = self.app.as_ref().map(|a| a.tab_manager.tabs()).unwrap_or(&[]);
        let _active_id = self.app.as_ref().and_then(|a| a.tab_manager.active_tab()).map(|t| t.id);

        self.tab_bar.height = TAB_BAR_HEIGHT;
        self.tab_bar.tab_layouts.clear();

        // "+" 新标签按钮位于标签栏右端
        let new_tab_x = screen_w - TAB_NEW_BTN_WIDTH;
        self.tab_bar.new_tab_rect = TabViewportRect {
            x: new_tab_x,
            y: 0.0,
            w: TAB_NEW_BTN_WIDTH,
            h: TAB_BAR_HEIGHT,
        };

        let tab_count = tabs.len();
        if tab_count == 0 {
            return;
        }

        // 标签填充红绿灯和"+"按钮之间的空间
        let tabs_available_w = new_tab_x - pureterm_ui::tab_bar::TRAFFIC_LIGHT_WIDTH;
        let equal_width = (tabs_available_w / tab_count as f32).max(TAB_MIN_WIDTH);

        let mut x = pureterm_ui::tab_bar::TRAFFIC_LIGHT_WIDTH;
        for tab in tabs {
            let close_width = if tab_count > 1 { TAB_CLOSE_BTN_WIDTH } else { 0.0 };

            let rect = TabViewportRect {
                x,
                y: 0.0,
                w: equal_width,
                h: TAB_BAR_HEIGHT,
            };

            let close_rect = if close_width > 0.0 {
                Some(TabViewportRect {
                    x: x + equal_width - close_width - 4.0,
                    y: 0.0,
                    w: close_width,
                    h: TAB_BAR_HEIGHT,
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
    fn pane_at_point(&self, px: f32, py: f32, screen_w: f32, screen_h: f32) -> Option<u64> {
        let layout = self.compute_layout(screen_w, screen_h);
        pane_at_point(&layout, px, py)
    }

    /// 将像素位置转换为特定面板的单元格坐标。
    fn pixel_to_cell_for_pane(&self, pane_id: u64, px: f32, py: f32, layout: &[(u64, PaneRect)]) -> (usize, usize) {
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
    fn screen_size(&self) -> (f32, f32) {
        self.renderer.as_ref()
            .and_then(|r| r.surface_config())
            .map(|c| (c.width as f32, c.height as f32))
            .unwrap_or((0.0, 0.0))
    }

    /// 调整所有面板大小以适应其分配的空间。
    fn resize_all_panes(&mut self) {
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

    /// 处理键盘输入 - 路由到聚焦面板。
    fn handle_key_input(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool, super_key: bool) {
        log::debug!("handle_key_input: key={:?}, ctrl={}, alt={}, shift={}, super={}", key, ctrl, alt, shift, super_key);

        // 全局热键在应用级别处理
        // 在 macOS 上，使用 Super（Command）键；在其他平台上，使用 Ctrl+Shift
        let copy_trigger = if cfg!(target_os = "macos") { super_key } else { ctrl && shift };
        let paste_trigger = if cfg!(target_os = "macos") { super_key } else { ctrl && shift };

        if copy_trigger && key == "c" && !ctrl {
            // 复制选区到系统剪贴板
            let text = self.focused_pane().and_then(|p| p.copy_selection());
            if let Some(text) = text {
                self.clipboard_text = text.clone();
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(&text);
                }
            }
            return;
        }
        if paste_trigger && key == "v" && !ctrl {
            // 从系统剪贴板粘贴
            let text = if let Ok(mut cb) = arboard::Clipboard::new() {
                cb.get_text().unwrap_or_else(|_| self.clipboard_text.clone())
            } else {
                self.clipboard_text.clone()
            };
            if let Some(ref mut pane) = self.focused_pane_mut() {
                if !text.is_empty() {
                    pane.paste(&text);
                }
            }
            return;
        }
        if ctrl && shift && key == "p" {
            self.profile_selector.visible = !self.profile_selector.visible;
            if let Some(ref window) = self.window {
                window.request_redraw();
            }
            return;
        }

        // 全选：macOS 上为 Cmd+A，其他地方为 Ctrl+Shift+A
        let select_all_trigger = if cfg!(target_os = "macos") { super_key } else { ctrl && shift };
        if select_all_trigger && key == "a" && !ctrl {
            if let Some(ref mut pane) = self.focused_pane_mut() {
                let rows = pane.terminal.rows();
                let cols = pane.terminal.cols();
                if rows > 0 && cols > 0 {
                    pane.selection = Some(Selection {
                        start_row: 0,
                        start_col: 0,
                        end_row: rows - 1,
                        end_col: cols - 1,
                        is_block: false,
                    });
                    pane.terminal.scroll_to_bottom();
                    pane.mark_dirty();
                }
            }
            if let Some(ref window) = self.window {
                window.request_redraw();
            }
            return;
        }

        // 委托给应用热键服务
        if let Some(ref mut app) = self.app {
            if app.handle_key(key, ctrl, alt, shift, super_key) {
                // 热键已被应用消耗
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
                return;
            }
        }

        if let Some(ref mut pane) = self.focused_pane_mut() {
            // 在 macOS 上，Command 键组合是应用级快捷键，
            // 不要将无法识别的组合转发到终端。
            if cfg!(target_os = "macos") && super_key {
                return;
            }
            pane.handle_key(key, ctrl, alt, shift);
        }
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    /// 处理面板动作（来自鼠标事件、上下文菜单等）
    fn handle_pane_action(&mut self, action: PaneAction) {
        match action {
            PaneAction::CopyText(text) => {
                self.clipboard_text = text.clone();
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Err(e) = cb.set_text(&text) {
                        log::warn!("Failed to write to clipboard: {}", e);
                    }
                }
            }
            PaneAction::PasteFromClipboard => {
                let text = if let Ok(mut cb) = arboard::Clipboard::new() {
                    cb.get_text().unwrap_or_else(|_| self.clipboard_text.clone())
                } else {
                    self.clipboard_text.clone()
                };
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    if !text.is_empty() {
                        pane.paste(&text);
                    }
                }
            }
            PaneAction::ShowContextMenu => {
                let pane_id = self.focused_pane_id;
                if let Some(pid) = pane_id {
                    self.show_context_menu(self.mouse_pos.0, self.mouse_pos.1, pid);
                }
            }
            PaneAction::OpenUrl(url) => {
                log::info!("Opening URL: {}", url);
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&url).spawn();
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("cmd").args(["/c", "start", &url]).spawn();
            }
            PaneAction::SplitRight => {
                self.handle_split(SplitSide::Right);
            }
            PaneAction::SplitLeft => {
                self.handle_split(SplitSide::Left);
            }
            PaneAction::SplitDown => {
                self.handle_split(SplitSide::Down);
            }
            PaneAction::SplitUp => {
                self.handle_split(SplitSide::Up);
            }
            PaneAction::EditTabTitle => {
                if let Some(ref app) = self.app {
                    if let Some(tab) = app.tab_manager.active_tab() {
                        self.rename_modal = Some(RenameModalState {
                            tab_id: tab.id,
                            new_title: tab.title.clone(),
                        });
                    }
                }
            }
            PaneAction::EditTabColor => {
                if let Some(ref app) = self.app {
                    if let Some(tab) = app.tab_manager.active_tab() {
                        self.color_modal = Some(ColorModalState {
                            tab_id: tab.id,
                            color: tab.color.clone().unwrap_or_else(|| "#808080".to_string()),
                        });
                    }
                }
            }
        }
    }

    /// 使用配置文件选择器中的配置文件进行连接。
    fn connect_with_profile(&mut self, profile: &Profile) {
        let (screen_w, screen_h) = self.screen_size();
        let renderer = match &self.renderer {
            Some(r) => r,
            None => return,
        };
        let (cell_w, cell_h) = renderer.cell_size();
        let cols = ((screen_w / cell_w).floor() as usize).max(1);
        let rows = ((screen_h / cell_h).floor() as usize).max(1);

        let params = profile_to_connect_params(profile);
        let title = profile_title(profile);

        let pane_id = self.create_pane_with_session(cols, rows, params);

        self.split_root = None;
        self.focused_pane_id = Some(pane_id);

        if let Some(ref mut app) = self.app {
            app.tab_manager.open_tab(
                TabContent::Pane(pane_id),
                title,
            );
        }

        // 打开标签页时隐藏欢迎页面
        self.welcome_page.visible = false;

        if let Some(ref window) = self.window {
            window.request_redraw();
        }

        log::info!("Connected to profile '{}' with pane {}", profile.name, pane_id);
    }

    /// 处理给定方向侧的分割动作。
    fn handle_split(&mut self, side: SplitSide) {
        let focused_id = match self.focused_pane_id {
            Some(id) => id,
            None => return,
        };

        let (screen_w, screen_h) = self.screen_size();
        let renderer = match &self.renderer {
            Some(r) => r,
            None => return,
        };
        let (cell_w, cell_h) = renderer.cell_size();
        let direction = side.direction();

        // 从聚焦面板的当前矩形计算新面板大小
        let layout = self.compute_layout(screen_w, screen_h);
        let focused_rect = layout.iter().find(|(id, _)| *id == focused_id)
            .map(|(_, r)| *r)
            .unwrap_or(PaneRect { x: 0.0, y: 0.0, w: screen_w, h: screen_h });

        // 新面板获得聚焦面板一半的空间
        let (new_cols, new_rows) = match direction {
            Direction::Vertical => {
                let new_w = focused_rect.w / 2.0;
                (((new_w / cell_w).floor() as usize).max(1),
                 ((focused_rect.h / cell_h).floor() as usize).max(1))
            }
            Direction::Horizontal => {
                let new_h = focused_rect.h / 2.0;
                (((focused_rect.w / cell_w).floor() as usize).max(1),
                 ((new_h / cell_h).floor() as usize).max(1))
            }
        };

        log::info!("Split {:?}: screen=({},{}) focused_rect=({},{},{},{}) new_pane_size=({},{})",
            side, screen_w, screen_h,
            focused_rect.x, focused_rect.y, focused_rect.w, focused_rect.h,
            new_cols, new_rows);

        let new_pane_id = self.create_pane(new_cols, new_rows);
        log::info!("Created new pane {}", new_pane_id);

        // 使用方向分割更新分割树
        match &mut self.split_root {
            None => {
                let mut container = SplitContainer::single(focused_id);
                container.split_pane_directional(focused_id, new_pane_id, side);
                self.split_root = Some(container);
            }
            Some(root) => {
                root.split_pane_directional(focused_id, new_pane_id, side);
            }
        }

        // 平均分配所有面板的空间
        if let Some(ref mut root) = self.split_root {
            root.equalize();
        }

        // 更新标签页内容
        if let Some(ref mut app) = self.app {
            if let Some(tab) = app.tab_manager.active_tab_mut() {
                tab.content = TabContent::Split(SplitRef {
                    root_id: self.split_root.as_ref().unwrap().id,
                });
            }
        }

        // 聚焦新面板
        self.focused_pane_id = Some(new_pane_id);

        // 调整所有面板大小以适应新布局
        self.resize_all_panes();

        // 记录新布局
        let new_layout = self.compute_layout(screen_w, screen_h);
        for (pid, rect) in &new_layout {
            log::info!("  Pane {} => rect ({},{},{},{})", pid, rect.x, rect.y, rect.w, rect.h);
        }
        log::info!("  Total panes: {}", self.panes.len());

        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    /// 处理关闭面板动作。
    fn handle_close_pane(&mut self, pane_id: u64) {
        if self.panes.len() <= 1 {
            return; // 不要关闭最后一个面板
        }

        if let Some(ref mut root) = self.split_root {
            root.close_pane(pane_id);
            if root.pane_count() <= 1 {
                let remaining = root.pane_ids();
                if let Some(&id) = remaining.first() {
                    self.focused_pane_id = Some(id);
                }
                self.split_root = None;
            }
        }

        self.panes.remove(&pane_id);

        // 如果关闭的面板处于聚焦状态，切换到另一个面板
        if self.focused_pane_id == Some(pane_id) {
            self.focused_pane_id = self.panes.keys().next().copied();
        }

        self.resize_all_panes();

        if let Some(ref window) = self.window {
            window.request_redraw();
        }

        log::info!("Closed pane {}", pane_id);
    }

    /// 处理上下文菜单动作。
    fn handle_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::Copy => {
                let text = self.focused_pane().and_then(|p| p.copy_selection());
                if let Some(text) = text {
                    self.clipboard_text = text.clone();
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        if let Err(e) = cb.set_text(&text) {
                            log::warn!("Failed to write to clipboard: {}", e);
                        }
                    }
                }
            }
            MenuAction::Paste => {
                let text = if let Ok(mut cb) = arboard::Clipboard::new() {
                    cb.get_text().unwrap_or_else(|_| self.clipboard_text.clone())
                } else {
                    self.clipboard_text.clone()
                };
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    if !text.is_empty() {
                        pane.paste(&text);
                    }
                }
            }
            MenuAction::SelectAll => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    let rows = pane.terminal.rows();
                    let cols = pane.terminal.cols();
                    if rows == 0 || cols == 0 {
                        return;
                    }
                    pane.selection = Some(Selection {
                        start_row: 0,
                        start_col: 0,
                        end_row: rows - 1,
                        end_col: cols - 1,
                        is_block: false,
                    });
                    pane.terminal.scroll_to_bottom();
                }
            }
            MenuAction::Clear => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.write_input(b"\x1b[2J\x1b[H");
                }
            }
            MenuAction::Search => {
                // 切换搜索模式：发送 Ctrl+F 到终端以触发 shell 反向搜索
                // 或仅显示视觉指示器
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.write_input(b"\x1b[1;5F"); // Ctrl+F
                }
            }
            MenuAction::SplitRight => {
                self.handle_split(SplitSide::Right);
            }
            MenuAction::SplitLeft => {
                self.handle_split(SplitSide::Left);
            }
            MenuAction::SplitDown => {
                self.handle_split(SplitSide::Down);
            }
            MenuAction::SplitUp => {
                self.handle_split(SplitSide::Up);
            }
            MenuAction::ClosePane => {
                if let Some(pane_id) = self.focused_pane_id {
                    self.handle_close_pane(pane_id);
                }
            }
            MenuAction::NewTab => {
                self.handle_new_tab();
            }
            MenuAction::CloseTab => {
                self.handle_close_tab();
            }
            MenuAction::ZoomIn => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.zoom_in();
                }
                self.resize_all_panes();
            }
            MenuAction::ZoomOut => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.zoom_out();
                }
                self.resize_all_panes();
            }
            MenuAction::ZoomReset => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.zoom_reset();
                }
                self.resize_all_panes();
            }
            MenuAction::OpenSettings => {
                self.settings_panel.visible = !self.settings_panel.visible;
                if let Some(ref mut app) = self.app {
                    app.show_settings = self.settings_panel.visible;
                }
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }
            MenuAction::EditTabTitle => {
                if let Some(ref app) = self.app {
                    if let Some(tab) = app.tab_manager.active_tab() {
                        self.rename_modal = Some(RenameModalState {
                            tab_id: tab.id,
                            new_title: tab.title.clone(),
                        });
                    }
                }
            }
            MenuAction::EditTabColor => {
                if let Some(ref app) = self.app {
                    if let Some(tab) = app.tab_manager.active_tab() {
                        self.color_modal = Some(ColorModalState {
                            tab_id: tab.id,
                            color: tab.color.clone().unwrap_or_else(|| "#808080".to_string()),
                        });
                    }
                }
            }
        }
    }

    /// 创建带有新终端面板的标签页。
    fn handle_new_tab(&mut self) {
        let (screen_w, screen_h) = self.screen_size();
        let renderer = match &self.renderer {
            Some(r) => r,
            None => return,
        };
        let (cell_w, cell_h) = renderer.cell_size();
        let cols = ((screen_w / cell_w).floor() as usize).max(1);
        let rows = ((screen_h / cell_h).floor() as usize).max(1);

        let pane_id = self.create_pane(cols, rows);

        // 关闭新标签页的当前分割状态
        self.split_root = None;
        self.focused_pane_id = Some(pane_id);

        if let Some(ref mut app) = self.app {
            let tab_count = app.tab_manager.len() + 1;
            app.tab_manager.open_tab(
                TabContent::Pane(pane_id),
                format!("Terminal {}", tab_count),
            );
        }

        // 打开标签页时隐藏欢迎页面
        self.welcome_page.visible = false;

        if let Some(ref window) = self.window {
            window.request_redraw();
        }

        log::info!("New tab created with pane {}", pane_id);
    }

    /// 关闭活动标签页并切换到最近的一个。
    fn handle_close_tab(&mut self) {
        // 关闭前获取活动标签页的面板 ID
        let tab_pane_ids: Vec<u64> = if let Some(ref app) = self.app {
            if let Some(tab) = app.tab_manager.active_tab() {
                match &tab.content {
                    TabContent::Pane(id) => vec![*id],
                    TabContent::Split(_root) => {
                        // 我们需要分割容器来获取所有面板 ID
                        // 目前，删除 self.panes 中属于此标签页的所有面板
                        // 由于我们没有单独跟踪哪些面板属于哪个标签页，
                        // 只从 split_root 收集所有面板 ID
                        self.split_root.as_ref()
                            .map(|root| root.pane_ids())
                            .unwrap_or_default()
                    }
                }
            } else {
                return;
            }
        } else {
            return;
        };

        // 关闭标签页
        if let Some(ref mut app) = self.app {
            app.tab_manager.close_active_tab();
        }

        // 删除属于已关闭标签页的面板
        for pane_id in &tab_pane_ids {
            self.panes.remove(pane_id);
        }
        self.split_root = None;

        // 切换到新活动标签页的面板
        if let Some(ref app) = self.app {
            if let Some(tab) = app.tab_manager.active_tab() {
                match &tab.content {
                    TabContent::Pane(id) => {
                        self.focused_pane_id = Some(*id);
                    }
                    TabContent::Split(_) => {
                        // 需要时恢复分割状态
                        self.focused_pane_id = self.panes.keys().next().copied();
                    }
                }
            } else {
                // 没有剩余标签页 - 创建新标签页
                drop(tab_pane_ids); // release borrow
                self.handle_new_tab();
                return;
            }
        }

        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    /// 使用渲染器的字形图集渲染上下文菜单覆盖层。
    fn render_context_menu_overlay(
        context_menu: &ContextMenuState,
        renderer: &mut TerminalRenderer,
        vertices: &mut Vec<pureterm_gui::pipeline::Vertex>,
        indices: &mut Vec<u32>,
    ) {
        if !context_menu.visible {
            return;
        }

        // 带圆角外观的背景
        let (mx, my, mw, mh) = context_menu.menu_rect();
        renderer.build_rect_vertices(
            ViewportRect { x: mx, y: my, w: mw, h: mh },
            [0.18, 0.18, 0.22, 0.95],
            vertices, indices,
        );

        let fg_color = [0.9f32, 0.9, 0.9, 1.0];
        let hover_bg = [0.25f32, 0.5, 0.85, 1.0];

        for (i, item, ix, iy) in context_menu.items_with_positions() {
            // 分隔符
            if item.separator_before {
                let sep_y = iy - MENU_SEPARATOR_HEIGHT + 2.0;
                renderer.build_rect_vertices(
                    ViewportRect { x: mx + 4.0, y: sep_y, w: mw - 8.0, h: 1.0 },
                    [0.4, 0.4, 0.4, 1.0],
                    vertices, indices,
                );
            }

            // 悬停高亮（整行）
            if context_menu.hovered_index == Some(i) {
                renderer.build_rect_vertices(
                    ViewportRect { x: mx, y: iy, w: mw, h: MENU_ITEM_HEIGHT },
                    hover_bg,
                    vertices, indices,
                );
            }

            // 文本
            renderer.build_text_vertices(
                &item.label,
                ix,
                iy + 4.0,
                fg_color,
                vertices, indices,
            );
        }

        // 菜单边框
        renderer.build_focus_border(
            ViewportRect { x: mx, y: my, w: mw, h: mh },
            [0.45, 0.45, 0.5, 1.0],
            1.0,
            vertices, indices,
        );
    }

    fn render_tab_bar_overlay(
        tab_bar: &TabBarState,
        active_tab_id: Option<u64>,
        renderer: &mut TerminalRenderer,
        color_scheme: &pureterm_core::theme::ColorScheme,
        vertices: &mut Vec<pureterm_gui::pipeline::Vertex>,
        indices: &mut Vec<u32>,
    ) {
        if !tab_bar.visible {
            return;
        }

        let screen_w: f32 = {
            let tabs_end = tab_bar.tab_layouts.iter().map(|l| l.rect.x + l.rect.w).fold(0.0, f32::max);
            let new_tab_end = tab_bar.new_tab_rect.x + tab_bar.new_tab_rect.w;
            tabs_end.max(new_tab_end)
        };

        // 从终端主题派生标签栏颜色（Ghostty 方式）
        let bg_rgb = pureterm_core::theme::ColorScheme::parse_color(&color_scheme.background);
        let fg_rgb = pureterm_core::theme::ColorScheme::parse_color(&color_scheme.foreground);
        let bar_bg = [bg_rgb[0] as f32 / 255.0, bg_rgb[1] as f32 / 255.0, bg_rgb[2] as f32 / 255.0, 1.0];

        // 标签栏背景 — 终端背景色略微变暗
        {
            let dim = 0.85;
            renderer.build_rect_vertices(
                ViewportRect { x: 0.0, y: 0.0, w: screen_w, h: TAB_BAR_HEIGHT },
                [bar_bg[0] * dim, bar_bg[1] * dim, bar_bg[2] * dim, 1.0],
                vertices, indices,
            );
        }

        // 底部边框 — 微妙的分隔符
        {
            let border = 0.25;
            renderer.build_rect_vertices(
                ViewportRect { x: 0.0, y: TAB_BAR_HEIGHT - 1.0, w: screen_w, h: 1.0 },
                [border, border, border + 0.03, 1.0],
                vertices, indices,
            );
        }

        // "+" 新标签按钮
        {
            let btn = tab_bar.new_tab_rect;
            if btn.w > 0.0 {
                // 悬停背景
                if tab_bar.hovered_new_tab {
                    let hover = 0.2;
                    renderer.build_rect_vertices(
                        ViewportRect { x: btn.x, y: btn.y, w: btn.w, h: btn.h },
                        [hover, hover, hover + 0.02, 1.0],
                        vertices, indices,
                    );
                }
                // "+" 符号居中
                let plus_y = (TAB_BAR_HEIGHT - 16.0) / 2.0;
                let plus_color = if tab_bar.hovered_new_tab {
                    [1.0f32, 1.0, 1.0, 1.0]
                } else {
                    [0.55f32, 0.55, 0.6, 1.0]
                };
                renderer.build_text_vertices(
                    "+",
                    btn.x + (btn.w - 12.0) / 2.0,
                    plus_y,
                    plus_color,
                    vertices, indices,
                );
            }
        }

        // 标签页
        let text_y = (TAB_BAR_HEIGHT - 16.0) / 2.0;

        for (i, layout) in tab_bar.tab_layouts.iter().enumerate() {
            let is_active = active_tab_id == Some(layout.tab_id);
            let is_hovered = tab_bar.hovered_tab_index == Some(i);

            // 标签页背景
            if is_active {
                // 活动标签页：终端背景色（与内容区域匹配）
                renderer.build_rect_vertices(
                    ViewportRect { x: layout.rect.x, y: layout.rect.y, w: layout.rect.w, h: layout.rect.h },
                    bar_bg,
                    vertices, indices,
                );
            } else if is_hovered {
                // 悬停的非活动标签：比标签栏略亮
                let f = 0.92;
                renderer.build_rect_vertices(
                    ViewportRect { x: layout.rect.x, y: layout.rect.y, w: layout.rect.w, h: layout.rect.h },
                    [bar_bg[0] * f + 0.05, bar_bg[1] * f + 0.05, bar_bg[2] * f + 0.05, 1.0],
                    vertices, indices,
                );
            }

            // 标签页标题文本 — Ghostty 使用 labelColor（活动）/ secondaryLabelColor（非活动）
            let available_width = layout.rect.w - TAB_PADDING_H;
            let max_text_width = available_width - TAB_CLOSE_BTN_WIDTH;
            let full_width = renderer.measure_text(&layout.title);

            let (display_title, text_width) = if full_width <= max_text_width {
                (layout.title.clone(), full_width)
            } else {
                let mut byte_end = 0usize;
                let mut w = 0.0f32;
                for (i, ch) in layout.title.char_indices() {
                    let cw = renderer.measure_text_char(ch);
                    if w + cw > max_text_width { break; }
                    w += cw;
                    byte_end = i + ch.len_utf8();
                }
                (layout.title[..byte_end].to_string(), w)
            };

            let title_x = layout.rect.x + (available_width - text_width) / 2.0;
            let fg_color = if is_active {
                [fg_rgb[0] as f32 / 255.0, fg_rgb[1] as f32 / 255.0, fg_rgb[2] as f32 / 255.0, 1.0]
            } else {
                // 非活动：暗淡的前景色（次要标签）
                let dim = 0.6;
                [fg_rgb[0] as f32 / 255.0 * dim, fg_rgb[1] as f32 / 255.0 * dim, fg_rgb[2] as f32 / 255.0 * dim, 1.0]
            };

            renderer.build_text_vertices(
                &display_title,
                title_x,
                text_y,
                fg_color,
                vertices, indices,
            );

            // 关闭按钮 — 仅在悬停时可见（Ghostty / 原生 macOS 行为）
            if let Some(ref close_rect) = layout.close_button_rect {
                if is_hovered {
                    // 悬停背景
                    let close_bg = if is_active { 0.25 } else { 0.18 };
                    renderer.build_rect_vertices(
                        ViewportRect {
                            x: close_rect.x,
                            y: 0.0,
                            w: close_rect.w,
                            h: TAB_BAR_HEIGHT,
                        },
                        [close_bg, close_bg * 0.6, close_bg * 0.6, 1.0],
                        vertices, indices,
                    );
                    // "x" 符号
                    renderer.build_text_vertices(
                        "x",
                        close_rect.x + (close_rect.w - 12.0) / 2.0,
                        text_y,
                        [0.9f32, 0.3, 0.3, 1.0],
                        vertices, indices,
                    );
                }
            }

            // 标签页颜色指示条（底部）
            if let Some(ref color_hex) = layout.color {
                let cv = parse_hex_color(color_hex);
                renderer.build_rect_vertices(
                    ViewportRect {
                        x: layout.rect.x,
                        y: layout.rect.y + layout.rect.h - 3.0,
                        w: layout.rect.w,
                        h: 3.0,
                    },
                    [cv[0] as f32 / 255.0, cv[1] as f32 / 255.0, cv[2] as f32 / 255.0, 1.0],
                    vertices, indices,
                );
            }
        }
    }
}

impl ApplicationHandler for PureTermApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let mut window_attrs = Window::default_attributes()
                .with_title("PureTerm")
                .with_inner_size(winit::dpi::LogicalSize::new(1024, 768));

            // 如果启用则应用 quake 模式设置
            if self.quake_mode.enabled {
                if let Some(monitor) = event_loop.available_monitors().next() {
                    let screen_size = monitor.size();
                    let quake_height = (screen_size.height as f32 * self.quake_mode.height) as u32;
                    window_attrs = window_attrs
                        .with_inner_size(winit::dpi::PhysicalSize::new(screen_size.width, quake_height))
                        .with_position(winit::dpi::PhysicalPosition::new(
                            monitor.position().x,
                            monitor.position().y,
                        ));
                    // 注意：always_on_top 需要平台特定的 API
                }
            }

            let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
            let size = window.inner_size();

            // 首先加载配置以获取外观设置
            let config = ConfigService::load().expect("Failed to load config");
            let appearance = &config.get().appearance;
            let terminal_config = &config.get().terminal;

            // Create transparency config from appearance settings
            let transparency = TransparencyConfig {
                opacity: appearance.opacity,
                blur: appearance.background_blur > 0,
                blur_radius: appearance.background_blur,
                transparent_background: !matches!(appearance.glass_effect, pureterm_core::config::GlassEffectStyle::None)
                    || appearance.opacity < 1.0,
            };

            // 如果配置了自定义字体路径，则使用它初始化渲染器
            let renderer = pollster::block_on(async {
                TerminalRenderer::new(terminal_config.font_path.as_deref()).await
            }).expect("Failed to initialize renderer");

            let surface = renderer.instance()
                .create_surface(Arc::clone(&window))
                .expect("Failed to create surface");

            let mut renderer = renderer;
            renderer.configure_surface(&surface, size.width, size.height, &transparency);
            renderer.set_corner_radius(appearance.corner_radius);

            // 应用平台特定的窗口样式（macOS：圆角、玻璃效果）
            #[cfg(target_os = "macos")]
            {
                platform::macos::apply_platform_styling(&window, appearance.corner_radius, &appearance.glass_effect);
            }

            // 初始化 egui 后端用于设置覆盖层
            let surface_format = renderer.surface_config()
                .map(|c| c.format)
                .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
            let egui_backend = EguiBackend::new(
                renderer.device(),
                surface_format,
                Arc::clone(&window),
            );

            // 将 PureTerm 主题应用到 egui 上下文（notedeck 启发的设计系统）
            apply_theme(&egui_backend.context, &config.get().appearance.ui_theme);

            let (cols, rows) = renderer.pixel_to_cells(size.width, size.height);

            // 创建初始面板
            let pane_id = self.create_pane(cols, rows);

            // 创建应用并打开初始标签页（使用已加载的配置）
            let mut app = App::new(config.get().clone());
            app.tab_manager.open_tab(
                TabContent::Pane(pane_id),
                "Terminal".to_string(),
            );

            self.focused_pane_id = Some(pane_id);
            self.app = Some(app);

            // 由于已有初始标签页，隐藏欢迎页面
            self.welcome_page.visible = false;
            self.renderer = Some(renderer);
            self.surface = Some(surface);
            self.window = Some(window);
            self.egui_backend = Some(egui_backend);
            self.appearance = appearance.clone();

            // 设置完成后请求初始渲染
            if let Some(ref win) = self.window {
                win.request_redraw();
            }

            log::info!("PureTerm initialized: {}x{}", cols, rows);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            // 当任何覆盖面板可见时，将事件路由到 egui
            ref evt @ WindowEvent::KeyboardInput { .. }
            | ref evt @ WindowEvent::MouseInput { .. }
            | ref evt @ WindowEvent::CursorMoved { .. }
            | ref evt @ WindowEvent::MouseWheel { .. }
            | ref evt @ WindowEvent::ModifiersChanged { .. }
            | ref evt @ WindowEvent::Focused { .. }
                if self.settings_panel.visible || self.profile_selector.visible || self.welcome_page.visible
                    || self.rename_modal.is_some() || self.color_modal.is_some() =>
            {
                if let (Some(ref mut egui_backend), Some(ref window)) = (&mut self.egui_backend, &self.window) {
                    let consumed = egui_backend.on_event(window, evt);
                    window.request_redraw();
                    if consumed {
                        return;
                    }
                }
            }
            _ => {}
        }

        match event {
            WindowEvent::CloseRequested => {
                log::info!("Window closed");
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let (Some(ref mut renderer), Some(ref mut surface)) =
                    (&mut self.renderer, &mut self.surface)
                {
                    if physical_size.width > 0 && physical_size.height > 0 {
                        let transparency = TransparencyConfig {
                            opacity: self.appearance.opacity,
                            blur: self.appearance.background_blur > 0,
                            blur_radius: self.appearance.background_blur,
                            transparent_background: !matches!(self.appearance.glass_effect, pureterm_core::config::GlassEffectStyle::None)
                                || self.appearance.opacity < 1.0,
                        };
                        renderer.configure_surface(surface, physical_size.width, physical_size.height, &transparency);
                        renderer.set_corner_radius(self.appearance.corner_radius);
                        self.resize_all_panes();

                        // macOS: 重新定位红绿灯按钮（窗口大小改变后 macOS 可能重置按钮位置）
                        #[cfg(target_os = "macos")]
                        if let Some(ref window) = self.window {
                            platform::macos::reposition_traffic_lights_on_resize(window);
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // 排出所有面板的输出
                for pane in self.panes.values_mut() {
                    pane.drain_output();
                }

                let any_dirty = self.panes.values().any(|p| p.is_dirty());
                log::debug!("RedrawRequested: any_dirty={}, context_menu={}, settings={}, profile={}, welcome={}, rename={}, color={}",
                    any_dirty, self.context_menu.visible, self.settings_panel.visible,
                    self.profile_selector.visible, self.welcome_page.visible,
                    self.rename_modal.is_some(), self.color_modal.is_some());
                if !any_dirty && !self.context_menu.visible && !self.settings_panel.visible && !self.profile_selector.visible && !self.welcome_page.visible && self.rename_modal.is_none() && self.color_modal.is_none() {
                    log::debug!("RedrawRequested: 由于没有脏面板且所有面板都隐藏，提前返回");
                    return;
                }

                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);
                let bg = pureterm_core::theme::ColorScheme::parse_color(&self.color_scheme.background);

                // 渲染前更新标签栏布局
                self.update_tab_bar_layout();

                // 在借用渲染器之前收集快照和选择状态
                let mut snapshots: Vec<(u64, ViewportRect, pureterm_terminal::ScreenSnapshot, Option<pureterm_ui::terminal::Selection>)> = Vec::new();
                for (pane_id, rect) in &layout {
                    if let Some(pane) = self.panes.get_mut(pane_id) {
                        let viewport = ViewportRect {
                            x: rect.x, y: rect.y,
                            w: rect.w, h: rect.h,
                        };
                        let snapshot = pane.terminal.snapshot();
                        let selection = pane.selection.clone();
                        log::debug!("Render pane {}: viewport=({},{},{},{}) grid=({}x{}) cursor=({},{}) visible={}",
                            pane_id, rect.x, rect.y, rect.w, rect.h,
                            snapshot.rows.len(), snapshot.rows.first().map(|r| r.len()).unwrap_or(0),
                            snapshot.cursor_row, snapshot.cursor_col, snapshot.cursor_visible);
                        pane.clear_dirty();
                        snapshots.push((*pane_id, viewport, snapshot, selection));
                    } else {
                        log::warn!("Layout contains pane {} but not found in panes map!", pane_id);
                    }
                }

                // 收集分隔符矩形
                let tab_bar_offset_y = if self.show_tab_bar() { TAB_BAR_HEIGHT } else { 0.0 };
                let dividers = self.split_root.as_ref()
                    .map(|root| compute_divider_rects(root, 0.0, tab_bar_offset_y, screen_w, screen_h - tab_bar_offset_y))
                    .unwrap_or_default();

                // 收集焦点边框信息
                let focus_rect = self.focused_pane_id
                    .and_then(|fid| layout.iter().find(|(id, _)| *id == fid).map(|(_, r)| *r));

                if let (Some(ref mut renderer), Some(ref surface)) = (&mut self.renderer, &self.surface) {
                    let color_scheme = self.color_scheme.clone();
                    let mut all_vertices = Vec::new();
                    let mut all_indices = Vec::new();

                    // 渲染每个面板快照
                    log::debug!("Rendering {} snapshots", snapshots.len());
                    for (_pane_id, viewport, snapshot, selection) in &snapshots {
                        let offset = all_vertices.len() as u32;
                        let sel = selection.clone();
                        let sel_cb: Option<Box<dyn Fn(usize, usize) -> bool>> = sel.map(|s| {
                            Box::new(move |row: usize, col: usize| s.contains(row, col))
                                as Box<dyn Fn(usize, usize) -> bool>
                        });
                        let (verts, idx) = renderer.build_vertices(
                            snapshot, &color_scheme, sel_cb.as_ref().map(|f| f.as_ref()), Some(*viewport),
                        );
                        log::debug!("  pane {}: verts={}, indices={}", _pane_id, verts.len(), idx.len());
                        all_vertices.extend(verts);
                        all_indices.extend(idx.iter().map(|i| i + offset));
                    }

                    // 渲染分隔符
                    for div in &dividers {
                        // 如果分隔符正在被拖拽或悬停，则高亮显示
                        let is_active = self.divider_drag.as_ref()
                            .map_or(false, |d| {
                                (d.divider_rect.x - div.x).abs() < 1.0
                                    && (d.divider_rect.y - div.y).abs() < 1.0
                            });
                        let is_hovered = self.hovered_divider.as_ref()
                            .map_or(false, |h| {
                                (h.rect.x - div.x).abs() < 1.0 && (h.rect.y - div.y).abs() < 1.0
                            });
                        let color = if is_active || is_hovered {
                            [0.4, 0.7, 1.0, 1.0]
                        } else {
                            [0.35, 0.35, 0.4, 1.0]
                        };
                        renderer.build_rect_vertices(
                            ViewportRect { x: div.x, y: div.y, w: div.w, h: div.h },
                            color,
                            &mut all_vertices, &mut all_indices,
                        );
                    }

                    // 渲染焦点边框
                    if let Some(rect) = focus_rect {
                        renderer.build_focus_border(
                            ViewportRect { x: rect.x, y: rect.y, w: rect.w, h: rect.h },
                            [0.25, 0.55, 0.85, 1.0],
                            2.0,
                            &mut all_vertices, &mut all_indices,
                        );
                    }

                    // 渲染标签栏覆盖层
                    let active_tab_id = self.app.as_ref().and_then(|a| a.tab_manager.active_tab()).map(|t| t.id);
                    Self::render_tab_bar_overlay(&self.tab_bar, active_tab_id, renderer, &self.color_scheme, &mut all_vertices, &mut all_indices);

                    // 渲染上下文菜单覆盖层
                    Self::render_context_menu_overlay(&self.context_menu, renderer, &mut all_vertices, &mut all_indices);

                    // 绘制终端内容（返回帧但不呈现）
                    let frame = renderer.draw_to_surface(surface, &all_vertices, &all_indices, bg);
                    if frame.is_none() {
                        log::warn!("draw_to_surface 返回 None，将在下一次重绘时重试");
                        // 重新标记所有面板为脏，以便重试渲染
                        for pane in self.panes.values_mut() {
                            pane.mark_dirty();
                        }
                        if let Some(ref window) = self.window {
                            window.request_redraw();
                        }
                        return;
                    }

                    // 如果任何面板/模态框可见，则渲染 egui 覆盖层
                    // 注意：欢迎页面仅在没有任何标签页时才需要渲染
                    let needs_egui = self.settings_panel.visible || self.profile_selector.visible
                        || self.rename_modal.is_some() || self.color_modal.is_some();
                    let show_welcome = self.welcome_page.visible && self.app.as_ref().map(|a| a.tab_manager.is_empty()).unwrap_or(false);
                    log::debug!("needs_egui={}, show_welcome={}, welcome_visible={}, app.tab_manager.is_empty()={}",
                        needs_egui, show_welcome, self.welcome_page.visible,
                        self.app.as_ref().map(|a| a.tab_manager.is_empty()).unwrap_or(false));
                    if needs_egui || show_welcome {
                        if let (Some(frame), Some(ref mut egui_backend), Some(ref mut app)) = (frame, &mut self.egui_backend, &mut self.app) {
                            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

                            // 提取 egui 闭包需要读写的状态
                            let settings_visible = self.settings_panel.visible;
                            let profile_visible = self.profile_selector.visible;
                            let welcome_visible = self.welcome_page.visible;
                            let mut profile_connect_request: Option<Profile> = None;
                            let rename_modal_rc = self.rename_modal.take().map(|m| Rc::new(RefCell::new(m)));
                            let color_modal_rc = self.color_modal.take().map(|m| Rc::new(RefCell::new(m)));

                            let raw_input = egui_backend.winit_state.take_egui_input(self.window.as_ref().unwrap());
                            let mut rename_done = false;
                            let mut color_done = false;
                            #[allow(deprecated)]
                            let full_output = egui_backend.run(raw_input, |ctx| {
                                // 设置面板
                                if settings_visible {
#[allow(deprecated)]
                                    egui::CentralPanel::default().show(ctx, |ui| {
                                        ui.vertical_centered(|ui| {
                                            ui.set_max_width(600.0);
                                            self.settings_panel.show(ui, &mut app.config);
                                        });
                                    });
                                }

                                // 配置文件选择器
                                if profile_visible {
                                    if let Some(selected_profile) = self.profile_selector.show(ctx, &app.config) {
                                        profile_connect_request = Some(selected_profile);
                                    }
                                }

                                // 欢迎页面 - 当没有打开标签页时显示
                                if welcome_visible && app.tab_manager.is_empty() {
                                    if let Some(selected_profile) = self.welcome_page.show_page(ctx, &app.config) {
                                        profile_connect_request = Some(selected_profile);
                                    }
                                }

                                // 重命名模态框
                                if let Some(ref rc_modal) = rename_modal_rc {
                                    let mut modal = rc_modal.borrow_mut();
                                    let tab_id = modal.tab_id;
                                    let mut close_ok = false;
                                    let mut close_cancel = false;
                                    egui::Window::new("Rename Tab")
                                        .collapsible(false)
                                        .resizable(false)
                                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                                        .show(ctx, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label("Title:");
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut modal.new_title)
                                                        .desired_width(200.0)
                                                );
                                            });
                                            let ok_clicked = ui.button("OK").clicked();
                                            let cancel_clicked = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                                            if ok_clicked {
                                                close_ok = true;
                                            }
                                            if cancel_clicked {
                                                close_cancel = true;
                                            }
                                        });
                                    if close_ok {
                                        self.rename_modal = Some(RenameModalState {
                                            tab_id,
                                            new_title: modal.new_title.clone(),
                                        });
                                        rename_done = true;
                                    } else if close_cancel {
                                        self.rename_modal = None;
                                        rename_done = true;
                                    }
                                }

                                // 颜色模态框
                                if let Some(ref rc_modal) = color_modal_rc {
                                    let mut modal = rc_modal.borrow_mut();
                                    let tab_id = modal.tab_id;
                                    let mut close_ok = false;
                                    let mut close_cancel = false;
                                    egui::Window::new("Tab Color")
                                        .collapsible(false)
                                        .resizable(false)
                                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                                        .show(ctx, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label("Color (#hex):");
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut modal.color)
                                                        .desired_width(120.0)
                                                );
                                            });
                                            // 颜色预览色块
                                            let color_vec = parse_hex_color(&modal.color);
                                            ui.horizontal(|ui| {
                                                ui.painter().rect_filled(
                                                    egui::Rect::from_min_size(ui.cursor().min, egui::vec2(40.0, 20.0)),
                                                    2.0,
                                                    egui::Color32::from_rgb(color_vec[0], color_vec[1], color_vec[2]),
                                                );
                                                ui.add_space(44.0);
                                            });
                                            ui.add_space(4.0);
                                            // 快速颜色预设
                                            ui.horizontal(|ui| {
                                                for &color in &["#e06c75", "#61afef", "#98c379", "#d19a66", "#c678dd", "#56b6c2", "#e5c07b", "#be5046"] {
                                                    let cv = parse_hex_color(color);
                                                    let (rect, response) = ui.allocate_exact_size(
                                                        egui::vec2(20.0, 20.0),
                                                        egui::Sense::click(),
                                                    );
                                                    ui.painter().rect_filled(
                                                        rect,
                                                        2.0,
                                                        egui::Color32::from_rgb(cv[0], cv[1], cv[2]),
                                                    );
                                                    if response.clicked() {
                                                        modal.color = color.to_string();
                                                    }
                                                }
                                            });
                                            ui.add_space(8.0);
                                            ui.horizontal(|ui| {
                                                if ui.button("OK").clicked() {
                                                    close_ok = true;
                                                }
                                                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                                    close_cancel = true;
                                                }
                                            });
                                        });
                                    if close_ok {
                                        self.color_modal = Some(ColorModalState {
                                            tab_id,
                                            color: modal.color.clone(),
                                        });
                                        color_done = true;
                                    } else if close_cancel {
                                        self.color_modal = None;
                                        color_done = true;
                                    }
                                }
                            });

                            // 应用模态框的结果
                            // 应用重命名模态框结果（仅在点击确定时设置）
                            if let Some(rename_state) = self.rename_modal.take() {
                                app.tab_manager.set_title(rename_state.tab_id, rename_state.new_title);
                            }
                            // 应用颜色模态框结果（仅在点击确定时设置）
                            if let Some(color_state) = self.color_modal.take() {
                                app.tab_manager.set_color(color_state.tab_id, Some(color_state.color));
                            }
                            // 如果仍然活跃则恢复模态框状态（既没有点击确定也没有点击取消）
                            if !rename_done && rename_modal_rc.is_some() {
                                let m = rename_modal_rc.as_ref().unwrap().borrow();
                                self.rename_modal = Some(RenameModalState {
                                    tab_id: m.tab_id,
                                    new_title: m.new_title.clone(),
                                });
                            }
                            if !color_done && color_modal_rc.is_some() {
                                let m = color_modal_rc.as_ref().unwrap().borrow();
                                self.color_modal = Some(ColorModalState {
                                    tab_id: m.tab_id,
                                    color: m.color.clone(),
                                });
                            }
                            // 检查主题是否改变并重新解析颜色方案
                            if app.config.appearance.theme != self.color_scheme.name {
                                let theme_service = pureterm_core::theme::ThemeService::new();
                                if let Some(scheme) = theme_service.get_scheme(&app.config.appearance.theme) {
                                    self.color_scheme = scheme.clone();
                                }
                            }

                            if let Some(profile) = profile_connect_request {
                                // 使用配置文件连接并请求重绘
                                drop(full_output);
                                drop(view);
                                self.connect_with_profile(&profile);
                                if let Some(ref window) = self.window {
                                    window.request_redraw();
                                }
                                return;
                            }

                            let shapes = full_output.shapes;
                            let textures_delta = full_output.textures_delta;
                            let pixels_per_point = self.window.as_ref().unwrap().scale_factor() as f32;
                            let paint_jobs = egui_backend.context.tessellate(shapes, pixels_per_point);

                            let screen_descriptor = egui_wgpu::ScreenDescriptor {
                                size_in_pixels: [screen_w as u32, screen_h as u32],
                                pixels_per_point,
                            };

                            egui_backend.render(
                                renderer.device(),
                                renderer.queue(),
                                &view,
                                paint_jobs,
                                textures_delta,
                                screen_descriptor,
                            );

                            frame.present();
                        }
                    } else if let Some(frame) = frame {
                        log::debug!("直接呈现帧 (needs_egui=false, show_welcome=false)");
                        frame.present();
                    } else {
                        log::warn!("没有要呈现的帧，也不需要 egui！");
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                log::debug!("ModifiersChanged: ctrl={}, alt={}, shift={}, super={}",
                    state.control_key(), state.alt_key(), state.shift_key(), state.super_key());
                self.modifiers = state;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // 仅处理按键按下事件，忽略释放
                if event.state != ElementState::Pressed {
                    return;
                }

                // 当模态框打开时跳过终端输入（让 egui 处理）
                if self.rename_modal.is_some() || self.color_modal.is_some() {
                    return;
                }

                use winit::keyboard::{Key, NamedKey};

                let ctrl = self.modifiers.control_key();
                let alt = self.modifiers.alt_key();
                let shift = self.modifiers.shift_key();
                let super_key = self.modifiers.super_key();

                // 全局热键：Ctrl+` 切换 quake 模式
                if ctrl && shift {
                    if let winit::keyboard::Key::Character(ref ch) = event.logical_key {
                        if ch == "`" {
                            // 切换 quake 模式可见性
                            if self.quake_mode.enabled {
                                self.quake_mode.toggle();
                                log::info!("Quake mode toggled: visible={}", self.quake_mode.visible);
                            }
                            return;
                        }
                    }
                }

                log::debug!("KeyboardInput: logical_key={:?}, text={:?}, ctrl={}, alt={}, shift={}",
                    event.logical_key, event.text, ctrl, alt, shift);

                // 处理带文本的字符输入
                if let Some(text) = event.text {
                    if !text.is_empty() {
                        let key = text.chars().next().unwrap();
                        if key == '\r' {
                            self.handle_key_input("Enter", ctrl, alt, shift, super_key);
                        } else if key == '\t' {
                            self.handle_key_input("Tab", ctrl, alt, shift, super_key);
                        } else if key == '\u{7f}' {
                            self.handle_key_input("Backspace", ctrl, alt, shift, super_key);
                        } else if key != '\u{1b}' { // Skip escape prefix
                            self.handle_key_input(&text.to_string(), ctrl, alt, shift, super_key);
                        }
                        return;
                    }
                }

                // 处理不带文本的键（特殊键、Ctrl+字母组合）
                match &event.logical_key {
                    Key::Named(named_key) => {
                        let key_str = match named_key {
                            NamedKey::Escape => "Escape",
                            NamedKey::Enter => "Enter",
                            NamedKey::Tab => "Tab",
                            NamedKey::Backspace => "Backspace",
                            NamedKey::ArrowUp => "Up",
                            NamedKey::ArrowDown => "Down",
                            NamedKey::ArrowRight => "Right",
                            NamedKey::ArrowLeft => "Left",
                            NamedKey::Home => "Home",
                            NamedKey::End => "End",
                            NamedKey::PageUp => "PageUp",
                            NamedKey::PageDown => "PageDown",
                            NamedKey::Insert => "Insert",
                            NamedKey::Delete => "Delete",
                            NamedKey::F1 => "F1",
                            NamedKey::F2 => "F2",
                            NamedKey::F3 => "F3",
                            NamedKey::F4 => "F4",
                            NamedKey::F5 => "F5",
                            NamedKey::F6 => "F6",
                            NamedKey::F7 => "F7",
                            NamedKey::F8 => "F8",
                            NamedKey::F9 => "F9",
                            NamedKey::F10 => "F10",
                            NamedKey::F11 => "F11",
                            NamedKey::F12 => "F12",
                            _ => return,
                        };
                        self.handle_key_input(key_str, ctrl, alt, shift, super_key);
                    }
                    Key::Character(ch) => {
                        self.handle_key_input(&ch.to_lowercase(), ctrl, alt, shift, super_key);
                    }
                    _ => {
                        log::debug!("KeyboardInput: 未处理的 logical_key={:?}", event.logical_key);
                    }
                }
            }

            // ── 鼠标事件 ──

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x as f32, position.y as f32);

                // 处理活动分隔符拖拽
                if let Some(ref drag) = self.divider_drag {
                    let (_screen_w, _screen_h) = self.screen_size();
                    let delta = match drag.direction {
                        Direction::Vertical => self.mouse_pos.0 - drag.start_pos.0,
                        Direction::Horizontal => self.mouse_pos.1 - drag.start_pos.1,
                    };
                    let delta_ratio = delta / drag.total_size;

                    let container_id = drag.container_id;
                    let child_index = drag.child_index;
                    if let Some(ref mut root) = self.split_root {
                        if let Some(container) = root.find_container_mut(container_id) {
                            container.adjust_ratio(child_index, delta_ratio);
                        }
                    }
                    // 更新 start_pos 以进行增量调整
                    if let Some(ref mut drag) = self.divider_drag {
                        drag.start_pos = self.mouse_pos;
                    }
                    self.resize_all_panes();
                    if let Some(ref window) = self.window {
                        window.request_redraw();
                    }
                    return;
                }

                // 更新上下文菜单悬停状态
                if self.context_menu.visible {
                    self.context_menu.update_hover(self.mouse_pos.0, self.mouse_pos.1);
                    if let Some(ref window) = self.window {
                        window.request_redraw();
                    }
                    return;
                }

                // 检查标签栏悬停状态
                if self.tab_bar.visible {
                    // 检查"+"新标签按钮
                    let hovered_new_tab = self.tab_bar.new_tab_rect.contains(self.mouse_pos.0, self.mouse_pos.1);

                    let hovered_idx = self.tab_bar.tab_layouts.iter().position(|l| {
                        l.rect.contains(self.mouse_pos.0, self.mouse_pos.1)
                    });

                    let changed = hovered_idx != self.tab_bar.hovered_tab_index
                        || hovered_new_tab != self.tab_bar.hovered_new_tab;
                    if changed {
                        self.tab_bar.hovered_tab_index = hovered_idx;
                        self.tab_bar.hovered_new_tab = hovered_new_tab;
                        if let Some(ref window) = self.window {
                            window.request_redraw();
                        }
                    }
                    if hovered_idx.is_some() || hovered_new_tab {
                        if let Some(ref window) = self.window {
                            window.set_cursor(winit::window::CursorIcon::Pointer);
                        }
                        return;
                    }
                }

                // 检查分隔符悬停（光标样式）
                let (screen_w, screen_h) = self.screen_size();
                self.hovered_divider = if let Some(ref root) = self.split_root {
                    divider_at_point(root, self.mouse_pos.0, self.mouse_pos.1, 0.0, 0.0, screen_w, screen_h)
                } else {
                    None
                };
                if let Some(ref window) = self.window {
                    let icon = if let Some(ref info) = self.hovered_divider {
                        match info.direction {
                            Direction::Vertical => winit::window::CursorIcon::ColResize,
                            Direction::Horizontal => winit::window::CursorIcon::RowResize,
                        }
                    } else {
                        winit::window::CursorIcon::Default
                    };
                    window.set_cursor(icon);
                }

                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);
                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);

                if let Some(target_id) = target_id {
                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                    let modifiers = MouseModifiers {
                        ctrl: self.modifiers.control_key(),
                        alt: self.modifiers.alt_key(),
                        shift: self.modifiers.shift_key(),
                    };

                    let actions: Vec<PaneAction> = {
                        let pane = self.panes.get_mut(&target_id);
                        if let Some(pane) = pane {
                            let event = TerminalMouseEvent::Move { row, col, modifiers };
                            let (to_send, action) = pane.handle_mouse_event(event);
                            if let Some(data) = to_send {
                                pane.write_input(&data);
                            }
                            if pane.is_dirty() {
                                if let Some(ref window) = self.window {
                                    window.request_redraw();
                                }
                            }
                            action.into_iter().collect()
                        } else {
                            Vec::new()
                        }
                    };
                    for action in actions {
                        self.handle_pane_action(action);
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);

                let mouse_button = match button {
                    WinitMouseButton::Left => MouseButton::Left,
                    WinitMouseButton::Middle => MouseButton::Middle,
                    WinitMouseButton::Right => MouseButton::Right,
                    _ => return,
                };

                match state {
                    ElementState::Pressed => {
                        match mouse_button {
                            MouseButton::Right => {
                                // Right click: show context menu
                                if self.context_menu.visible {
                                    self.context_menu.hide();
                                }

                                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                                if let Some(target_id) = target_id {
                                    self.focused_pane_id = Some(target_id);

                                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                                    let modifiers = MouseModifiers {
                                        ctrl: self.modifiers.control_key(),
                                        alt: self.modifiers.alt_key(),
                                        shift: self.modifiers.shift_key(),
                                    };

                                    let actions: Vec<PaneAction> = {
                                        let pane = self.panes.get_mut(&target_id);
                                        if let Some(pane) = pane {
                                            pane.terminal.scroll_to_bottom();
                                            let event = TerminalMouseEvent::Down { button: MouseButton::Right, row, col, modifiers };
                                            let (to_send, action) = pane.handle_mouse_event(event);
                                            if let Some(data) = to_send {
                                                pane.write_input(&data);
                                            }
                                            action.into_iter().collect()
                                        } else {
                                            Vec::new()
                                        }
                                    };
                                    for action in actions {
                                        self.handle_pane_action(action);
                                    }
                                }

                                // Show context menu
                                if let Some(target_id) = self.focused_pane_id.or_else(|| self.panes.keys().next().copied()) {
                                    self.show_context_menu(self.mouse_pos.0, self.mouse_pos.1, target_id);
                                }
                            }
                            MouseButton::Left => {
                                // 左键点击

                                // 首先检查上下文菜单（如果可见且点击命中）
                                if self.context_menu.visible {
                                    if let Some(action) = self.context_menu.hit_test(self.mouse_pos.0, self.mouse_pos.1) {
                                        self.context_menu.hide();
                                        self.handle_menu_action(action);
                                        if let Some(ref window) = self.window {
                                            window.request_redraw();
                                        }
                                        return;
                                    }
                                    // 点击在菜单外部 - 隐藏菜单并继续处理标签栏/其他
                                    self.context_menu.hide();
                                }

                                // 检查是否点击了标签栏
                                if self.tab_bar.visible {
                                    // macOS: 跳过红绿灯区域，让系统原生处理关闭/最小化/最大化
                                    #[cfg(target_os = "macos")]
                                    if self.mouse_pos.0 < TRAFFIC_LIGHT_WIDTH && self.mouse_pos.1 < TAB_BAR_HEIGHT {
                                        return;
                                    }

                                    if let Some(idx) = self.tab_bar.tab_layouts.iter().position(|l| l.rect.contains(self.mouse_pos.0, self.mouse_pos.1)) {
                                        let layout = &self.tab_bar.tab_layouts[idx];
                                        let tab_id = layout.tab_id;

                                        // 检查是否点击了关闭按钮
                                        if let Some(ref close_rect) = layout.close_button_rect {
                                            if close_rect.contains(self.mouse_pos.0, self.mouse_pos.1) {
                                                // 关闭此标签页
                                                if let Some(ref mut app) = self.app {
                                                    app.tab_manager.close_tab(tab_id);
                                                }
                                                if let Some(ref window) = self.window {
                                                    window.request_redraw();
                                                }
                                                return;
                                            }
                                        }

                                        // 切换到此标签页
                                        let current_active = self.app.as_ref().and_then(|a| a.tab_manager.active_tab()).map(|t| t.id);
                                        if current_active != Some(tab_id) {
                                            if let Some(ref mut app) = self.app {
                                                app.tab_manager.select_tab(tab_id);
                                                // 从标签页内容更新 focused_pane_id
                                                if let Some(tab) = app.tab_manager.active_tab() {
                                                    match &tab.content {
                                                        TabContent::Pane(pane_id) => {
                                                            self.focused_pane_id = Some(*pane_id);
                                                            self.split_root = None;
                                                            if let Some(pane) = self.panes.get_mut(pane_id) {
                                                                pane.mark_dirty();
                                                            }
                                                        }
                                                        TabContent::Split(split_ref) => {
                                                            // TODO: restore split state per tab
                                                            let _ = split_ref;
                                                        }
                                                    }
                                                }
                                            }
                                            if let Some(ref window) = self.window {
                                                window.request_redraw();
                                            }
                                        }
                                        return;
                                    }

                                    // 检查是否点击了"+"新标签按钮
                                    if self.tab_bar.new_tab_rect.contains(self.mouse_pos.0, self.mouse_pos.1) {
                                        self.handle_new_tab();
                                        return;
                                    }
                                }

                                // 检查是否点击了分隔符
                                if let Some(ref root) = self.split_root {
                                    if let Some(info) = divider_at_point(root, self.mouse_pos.0, self.mouse_pos.1, 0.0, 0.0, screen_w, screen_h) {
                                        self.divider_drag = Some(DividerDrag {
                                            container_id: info.container_id,
                                            child_index: info.child_index,
                                            direction: info.direction,
                                            start_pos: self.mouse_pos,
                                            total_size: info.container_size,
                                            divider_rect: PaneRect {
                                                x: info.rect.x,
                                                y: info.rect.y,
                                                w: info.rect.w,
                                                h: info.rect.h,
                                            },
                                        });
                                        return;
                                    }
                                }

                                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                                if let Some(target_id) = target_id {
                                    self.focused_pane_id = Some(target_id);
                                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                                    let modifiers = MouseModifiers {
                                        ctrl: self.modifiers.control_key(),
                                        alt: self.modifiers.alt_key(),
                                        shift: self.modifiers.shift_key(),
                                    };
                                    let ctrl = modifiers.ctrl;

                                    let actions: Vec<PaneAction> = {
                                        let pane = self.panes.get_mut(&target_id);
                                        if let Some(pane) = pane {
                                            pane.terminal.scroll_to_bottom();
                                            let event = TerminalMouseEvent::Down { button: MouseButton::Left, row, col, modifiers };
                                            let (to_send, action) = pane.handle_mouse_event(event);
                                            if let Some(data) = to_send {
                                                pane.write_input(&data);
                                            }
                                            let mut acts: Vec<PaneAction> = action.into_iter().collect();
                                            // 处理 Ctrl+点击 URL/超链接
                                            if ctrl {
                                                if let Some(url) = pane.url_at(row, col) {
                                                    acts.push(PaneAction::OpenUrl(url));
                                                }
                                            }
                                            if pane.is_dirty() {
                                                if let Some(ref window) = self.window {
                                                    window.request_redraw();
                                                }
                                            }
                                            acts
                                        } else {
                                            Vec::new()
                                        }
                                    };
                                    for action in actions {
                                        self.handle_pane_action(action);
                                    }
                                }

                                if let Some(ref window) = self.window {
                                    window.request_redraw();
                                }
                            }
                            MouseButton::Middle => {
                                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                                if let Some(target_id) = target_id {
                                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                                    let modifiers = MouseModifiers {
                                        ctrl: self.modifiers.control_key(),
                                        alt: self.modifiers.alt_key(),
                                        shift: self.modifiers.shift_key(),
                                    };

                                    let actions: Vec<PaneAction> = {
                                        let pane = self.panes.get_mut(&target_id);
                                        if let Some(pane) = pane {
                                            let event = TerminalMouseEvent::Down { button: MouseButton::Middle, row, col, modifiers };
                                            let (to_send, action) = pane.handle_mouse_event(event);
                                            if let Some(data) = to_send {
                                                pane.write_input(&data);
                                            }
                                            action.into_iter().collect()
                                        } else {
                                            Vec::new()
                                        }
                                    };
                                    for action in actions {
                                        self.handle_pane_action(action);
                                    }
                                }
                            }
                        }
                    }
                    ElementState::Released => {
                        // 结束分隔符拖拽
                        if self.divider_drag.is_some() {
                            self.divider_drag = None;
                            return;
                        }

                        if self.context_menu.visible {
                            return;
                        }

                        let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                        if let Some(target_id) = target_id {
                            let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                            let modifiers = MouseModifiers {
                                ctrl: self.modifiers.control_key(),
                                alt: self.modifiers.alt_key(),
                                shift: self.modifiers.shift_key(),
                            };

                            let actions: Vec<PaneAction> = {
                                let pane = self.panes.get_mut(&target_id);
                                if let Some(pane) = pane {
                                    let event = TerminalMouseEvent::Up { button: mouse_button, row, col, modifiers };
                                    let (to_send, action) = pane.handle_mouse_event(event);
                                    if let Some(data) = to_send {
                                        pane.write_input(&data);
                                    }
                                    if pane.is_dirty() {
                                        if let Some(ref window) = self.window {
                                            window.request_redraw();
                                        }
                                    }
                                    action.into_iter().collect()
                                } else {
                                    Vec::new()
                                }
                            };
                            for action in actions {
                                self.handle_pane_action(action);
                            }
                        }
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if self.context_menu.visible {
                    return;
                }

                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);

                let delta_rows = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y as i32,
                    MouseScrollDelta::PixelDelta(pos) => {
                        -(pos.y / 40.0) as i32
                    }
                };

                if delta_rows == 0 {
                    return;
                }

                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                if let Some(target_id) = target_id {
                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                    let modifiers = MouseModifiers {
                        ctrl: self.modifiers.control_key(),
                        alt: self.modifiers.alt_key(),
                        shift: self.modifiers.shift_key(),
                    };

                    let actions: Vec<PaneAction> = {
                        let pane = self.panes.get_mut(&target_id);
                        if let Some(pane) = pane {
                            let event = TerminalMouseEvent::Scroll { delta_rows, row, col, modifiers };
                            let (to_send, action) = pane.handle_mouse_event(event);
                            if let Some(data) = to_send {
                                pane.write_input(&data);
                            }
                            if pane.is_dirty() {
                                if let Some(ref window) = self.window {
                                    window.request_redraw();
                                }
                            }
                            action.into_iter().collect()
                        } else {
                            Vec::new()
                        }
                    };
                    for action in actions {
                        self.handle_pane_action(action);
                    }
                }
            }

            // ── 拖放 ──

            WindowEvent::DroppedFile(path) => {
                let path_str = path.to_string_lossy().to_string();
                log::info!("File dropped: {}", path_str);
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.handle_dropped_file(&path_str);
                }
            }

            WindowEvent::HoveredFile(path) => {
                log::debug!("File hovered: {:?}", path);
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // 不要无条件地在这里请求重绘 - 这会导致 RedrawRequested 事件
        // 堆积并阻塞其他事件（如鼠标点击）的处理。
        // 相反，只在实际情况发生变化时才请求重绘。
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // 被 PTY 输出线程唤醒 — 请求重绘以显示新的终端内容。
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

use clap::{Parser, Subcommand};

/// PureTerm CLI 参数。
#[derive(Parser, Debug)]
#[command(name = "pureterm")]
#[command(version = "0.1.0")]
#[command(about = "GPU-accelerated terminal emulator")]
struct Cli {
    /// 通过名称连接到配置文件。
    #[arg(short, long)]
    profile: Option<String>,

    /// 快速 SSH 连接（user@host 或 user@host:port）。
    #[arg(short, long)]
    ssh: Option<String>,

    /// 打开新标签页（用于 IPC）。
    #[arg(long)]
    new_tab: bool,

    /// 启动 quake 模式（下拉终端）。
    #[arg(long)]
    quake: bool,

    /// 用于额外操作的子命令。
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 列出可用配置文件。
    List,
    /// 打开设置。
    Settings,
    /// 列出所有可用字体。
    ListFonts,
    /// 列出所有可用主题。
    ListThemes,
    /// 显示当前配置。
    ShowConfig,
    /// 显示版本信息。
    Version,
}

fn main() -> Result<()> {
    #[cfg(debug_assertions)]
    let default_level = "debug";
    #[cfg(not(debug_assertions))]
    let default_level = "warn";

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(default_level)
    ).init();

    // 初始化崩溃报告（仅本地存储，不发送到网络）
    // 使用本地崩溃目录
    let crash_dir = std::env::temp_dir().join("pureterm-crashes");
    std::fs::create_dir_all(&crash_dir).ok();

    // 初始化 sentry - 空 DSN 禁用网络传输
    let _guard = sentry::init(sentry::ClientOptions {
        dsn: None,
        release: Some(env!("CARGO_PKG_VERSION").into()),
        ..Default::default()
    });
    // 设置 panic 钩子以捕获崩溃
    std::panic::set_hook(Box::new(|panic_info| {
        let payload = panic_info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };
        let location = panic_info.location().map(|loc| {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
        }).unwrap_or_else(|| "unknown".to_string());
        eprintln!("PANIC: {} at {}", message, location);
        sentry::capture_message(&format!("Panic: {} at {}", message, location), sentry::Level::Error);
    }));

    log::info!("PureTerm starting... (build: {})", if cfg!(debug_assertions) { "debug" } else { "release" });

    let cli = Cli::parse();

    // 处理子命令
    match cli.command {
        Some(Commands::List) => {
            println!("Available profiles:");
            // 在这里加载并列出配置文件
            println!("  (use --profile to connect)");
            return Ok(());
        }
        Some(Commands::Settings) => {
            println!("Opening settings...");
            // 在这里触发设置打开
            return Ok(());
        }
        Some(Commands::ListFonts) => {
            println!("Available fonts:");
            println!("  JetBrainsMono Nerd Font (embedded)");
            println!("  (custom fonts can be configured in settings)"); // (custom fonts can be configured in settings)
            return Ok(());
        }
        Some(Commands::ListThemes) => {
            use pureterm_core::theme::builtin_color_schemes;
            println!("Available themes:");
            for scheme in builtin_color_schemes() {
                println!("  {}", scheme.name);
            }
            return Ok(());
        }
        Some(Commands::ShowConfig) => {
            let config_service = ConfigService::load()?;
            let config = config_service.get();
            println!("Current configuration:");
            println!("{}", serde_json::to_string_pretty(&*config).unwrap_or_default());
            return Ok(());
        }
        Some(Commands::Version) => {
            println!("PureTerm {}", env!("CARGO_PKG_VERSION"));
            println!("Built with: Rust, wgpu, egui");
            println!("Features: GPU-accelerated rendering, ligatures, unicode box-drawing");
            return Ok(());
        }
        None => {}
    }

    let config_service = ConfigService::load()?;
    let theme_service = ThemeService::new();
    let mut config = config_service.get().clone();

    // 处理快速 SSH 连接
    if let Some(ssh_spec) = cli.ssh {
        if let Some(profile) = parse_ssh_spec(&ssh_spec) {
            config.profiles.push(profile);
        }
    }

    let color_scheme = config.terminal.color_scheme.clone()
        .or_else(|| theme_service.get_scheme(&config.appearance.theme).cloned())
        .unwrap_or_else(|| theme_service.default_scheme().clone());

    let event_loop = winit::event_loop::EventLoop::new()?;
    let proxy = event_loop.create_proxy();
    let quake_mode = cli.quake;

    let mut app = PureTermApp {
        window: None,
        renderer: None,
        surface: None,
        app: None,
        panes: HashMap::new(),
        split_root: None,
        focused_pane_id: None,
        context_menu: ContextMenuState::new(),
        color_scheme,
        modifiers: ModifiersState::default(),
        mouse_pos: (0.0, 0.0),
        clipboard_text: String::new(),
        divider_drag: None,
        hovered_divider: None,
        egui_backend: None,
        settings_panel: SettingsPanel::default(),
        profile_selector: ProfileSelector::default(),
        welcome_page: WelcomePage::new(),
        rename_modal: None,
        color_modal: None,
        quake_mode: if quake_mode { QuakeMode::new() } else { QuakeMode::default() },
        tab_bar: TabBarState::new(),
        appearance: pureterm_core::config::AppearanceConfig::default(),
        proxy,
    };

    event_loop.run_app(&mut app)?;

    log::info!("PureTerm shutting down");
    Ok(())
}

/// 将 SSH 规范字符串（user@host:port）解析为 Profile。
fn parse_ssh_spec(spec: &str) -> Option<Profile> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }

    let (user, host_part) = if let Some(at) = spec.find('@') {
        (Some(&spec[..at]), &spec[at + 1..])
    } else {
        (None, spec)
    };

    let (host, port) = if let Some(colon) = host_part.find(':') {
        let port: u16 = host_part[colon + 1..].parse().ok()?;
        (host_part[..colon].to_string(), Some(port))
    } else {
        (host_part.to_string(), None)
    };

    let user = user.map(|s| s.to_string())
        .or_else(|| std::env::var("USER").ok());

    Some(Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: format!("{}@{}", user.as_deref().unwrap_or("user"), host),
        profile_type: ProfileType::Ssh,
        group: None,
        color: Some("#4A9EFF".to_string()),
        icon: None,
        options: ProfileOptions {
            host: Some(host),
            port,
            user,
            auth_method: Some("prompt".to_string()),
            ..Default::default()
        },
    })
}

/// 将 Profile 转换为 ConnectParams 以创建会话。
fn profile_to_connect_params(profile: &Profile) -> ConnectParams {
    let session_type = match profile.profile_type {
        ProfileType::Local => {
            SessionType::Local {
                shell: profile.options.shell.clone(),
                args: profile.options.args.clone().unwrap_or_default(),
            }
        }
        ProfileType::Ssh => {
            let auth = match profile.options.auth_method.as_deref() {
                Some("password") => SshAuth::Password(String::new()),
                Some("publickey") => {
                    let key_path = profile.options.private_key.clone()
                        .unwrap_or_else(|| "~/.ssh/id_rsa".to_string());
                    SshAuth::PublicKey {
                        key_path: std::path::PathBuf::from(key_path),
                        passphrase: None,
                    }
                }
                Some("agent") => SshAuth::Agent,
                Some("none") => SshAuth::None,
                _ => SshAuth::PromptPassword,
            };
            SessionType::Ssh {
                host: profile.options.host.clone().unwrap_or_default(),
                port: profile.options.port.unwrap_or(22),
                user: profile.options.user.clone().unwrap_or_default(),
                auth,
                agent_forward: profile.options.agent_forward.unwrap_or(false),
                x11_forward: profile.options.x11_forward.unwrap_or(false),
                compression: profile.options.compression.unwrap_or(false),
            }
        }
        ProfileType::Serial => {
            SessionType::Serial {
                path: profile.options.path.clone().unwrap_or_default(),
                baud_rate: profile.options.baud_rate.unwrap_or(9600),
                data_bits: profile.options.data_bits.unwrap_or(8),
                stop_bits: profile.options.stop_bits.unwrap_or(1),
                parity: profile.options.parity.clone().unwrap_or_else(|| "none".into()),
                flow_control: profile.options.flow_control.clone().unwrap_or_else(|| "none".into()),
            }
        }
        ProfileType::Telnet => {
            SessionType::Telnet {
                host: profile.options.telnet_host.clone().unwrap_or_default(),
                port: profile.options.telnet_port.unwrap_or(23),
            }
        }
    };

    ConnectParams {
        session_type,
        env: profile.options.env.clone().unwrap_or_default(),
        cwd: profile.options.cwd.clone(),
    }
}

/// 从配置文件生成标签页标题。
fn profile_title(profile: &Profile) -> String {
    match profile.profile_type {
        ProfileType::Ssh => {
            let user = profile.options.user.as_deref().unwrap_or("user");
            let host = profile.options.host.as_deref().unwrap_or("host");
            let port = profile.options.port.unwrap_or(22);
            if port == 22 { format!("{}@{}", user, host) } else { format!("{}@{}:{}", user, host, port) }
        }
        ProfileType::Serial => {
            let path = profile.options.path.as_deref().unwrap_or("/dev/tty");
            let baud = profile.options.baud_rate.unwrap_or(9600);
            format!("{} @ {}", path, baud)
        }
        ProfileType::Telnet => {
            let host = profile.options.telnet_host.as_deref().unwrap_or("host");
            let port = profile.options.telnet_port.unwrap_or(23);
            if port == 23 { host.to_string() } else { format!("{}:{}", host, port) }
        }
        ProfileType::Local => profile.name.clone(),
    }
}

/// 将十六进制颜色字符串（如 "#ff0000"）解析为 [u8; 3]。
fn parse_hex_color(s: &str) -> [u8; 3] {
    let s = s.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(128);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(128);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(128);
        [r, g, b]
    } else {
        [128, 128, 128]
    }
}
