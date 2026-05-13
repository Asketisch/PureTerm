use pureterm_ui::context_menu::MenuAction;
use pureterm_ui::tabs::TabContent;
use pureterm_ui::terminal::pane::Selection;
use pureterm_ui::split::SplitSide;

use crate::app::{PureTermApp, RenameModalState, ColorModalState};

impl PureTermApp {
    /// 处理上下文菜单动作。
    pub(crate) fn handle_menu_action(&mut self, action: MenuAction) {
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
    pub(crate) fn handle_new_tab(&mut self) {
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
        self.tab_bar.tab_bar_dirty = true;

        if let Some(ref window) = self.window {
            window.request_redraw();
        }

        log::info!("New tab created with pane {}", pane_id);
    }

    /// 关闭活动标签页并切换到最近的一个。
    pub(crate) fn handle_close_tab(&mut self) {
        let active_id = self.app.as_ref().and_then(|a| a.tab_manager.active_tab()).map(|t| t.id);
        if let Some(id) = active_id {
            self.close_tab_by_id(id);
        }
    }

    /// 通过 ID 关闭标签页，清理面板并切换到新的活动标签页。
    pub(crate) fn close_tab_by_id(&mut self, tab_id: u64) {
        // 关闭前获取该标签页的面板 ID
        let tab_pane_ids: Vec<u64> = if let Some(ref app) = self.app {
            let tabs = app.tab_manager.tabs();
            if let Some(tab) = tabs.iter().find(|t| t.id == tab_id) {
                match &tab.content {
                    TabContent::Pane(id) => vec![*id],
                    TabContent::Split(_root) => {
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
            app.tab_manager.close_tab(tab_id);
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
                        self.focused_pane_id = self.panes.keys().next().copied();
                    }
                }
            } else {
                // 没有剩余标签页 - 创建新标签页
                drop(tab_pane_ids);
                self.tab_bar.tab_bar_dirty = true;
                self.handle_new_tab();
                return;
            }
        }

        self.tab_bar.tab_bar_dirty = true;
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}
