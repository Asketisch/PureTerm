use std::sync::Arc;

use pureterm_core::profile::Profile;
use pureterm_session::base::{ConnectParams, SessionType, SshAuth};
use pureterm_session::Session;
use pureterm_session::LocalSession;
use pureterm_session::SshSession;
use pureterm_session::SerialSession;
use pureterm_session::TelnetSession;
use pureterm_ui::split::{SplitSide, SplitContainer, Direction, PaneRect};
use pureterm_ui::tabs::TabContent;
use pureterm_ui::terminal::pane::{TerminalPane, PaneAction};
use pureterm_terminal::mark::MarkKind;

use crate::app::{PureTermApp, RenameModalState, ColorModalState};
use crate::cli::{profile_to_connect_params, profile_title};

impl PureTermApp {
    /// 创建带有本地会话的新终端面板（便捷方法）。
    pub(crate) fn create_pane(&mut self, cols: usize, rows: usize) -> u64 {
        self.create_pane_with_session(cols, rows, ConnectParams {
            session_type: SessionType::Local { shell: None, args: Vec::new() },
            env: std::collections::HashMap::new(),
            cwd: None,
        })
    }

    /// 使用给定会话类型创建新的终端面板。
    pub(crate) fn create_pane_with_session(&mut self, cols: usize, rows: usize, params: ConnectParams) -> u64 {
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
            SessionType::Tmux { session_name, attach, socket_path } => {
                let config = pureterm_session::TmuxSessionConfig {
                    session_name: session_name.clone(),
                    attach: *attach,
                    socket_path: socket_path.clone(),
                    extra_flags: Vec::new(),
                };
                Box::new(
                    pureterm_session::TmuxSession::new(params.clone(), config)
                        .expect("Failed to create tmux session")
                )
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
        pane.set_trigger_service(self.trigger_service.clone());
        pane.set_auto_profile(self.auto_profile_switcher.clone());
        pane.set_clipboard_handler(Box::new(|text: &str| {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                if let Err(e) = cb.set_text(text) {
                    log::warn!("OSC 52 clipboard: {}", e);
                }
            }
        }));

        // Set up mark callback for OSC 133 shell integration.
        // When a CommandStart mark is received, prevent sleep.
        // When an OutputEnd mark is received, allow sleep.
        let pending_marks = Arc::clone(&self.pending_marks);
        let mark_pane_id = pane_id;
        pane.terminal.set_mark_callback(Box::new(move |kind: MarkKind| {
            if let Ok(mut marks) = pending_marks.lock() {
                marks.push((mark_pane_id, kind));
            }
        }));

        self.panes.insert(pane_id, pane);
        pane_id
    }

    /// 处理面板动作（来自鼠标事件、上下文菜单等）
    pub(crate) fn handle_pane_action(&mut self, action: PaneAction) {
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
    pub(crate) fn connect_with_profile(&mut self, profile: &Profile) {
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
    pub(crate) fn handle_split(&mut self, side: SplitSide) {
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
                tab.content = TabContent::Split(pureterm_ui::tabs::SplitRef {
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
    pub(crate) fn handle_close_pane(&mut self, pane_id: u64) {
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
}
