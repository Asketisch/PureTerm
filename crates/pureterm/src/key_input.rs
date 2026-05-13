use pureterm_ui::terminal::pane::Selection;

use crate::app::PureTermApp;

impl PureTermApp {
    /// 处理键盘输入 - 路由到聚焦面板。
    pub(crate) fn handle_key_input(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool, super_key: bool) {
        log::debug!("handle_key_input: key={:?}, ctrl={}, alt={}, shift={}, super={}", key, ctrl, alt, shift, super_key);

        // ── Popup overlay navigation (autocomplete & history) ──
        // When a popup is visible, intercept navigation keys before they reach the terminal.
        {
            let popup_active = self.focused_pane().map_or(false, |p| {
                p.autocomplete_visible || p.history_visible
            });
            if popup_active {
                match key {
                    "Up" => {
                        if let Some(ref mut pane) = self.focused_pane_mut() {
                            if pane.autocomplete_visible {
                                pane.autocomplete_up();
                            } else if pane.history_visible {
                                pane.history_prev();
                            }
                        }
                        if let Some(ref window) = self.window { window.request_redraw(); }
                        return;
                    }
                    "Down" => {
                        if let Some(ref mut pane) = self.focused_pane_mut() {
                            if pane.autocomplete_visible {
                                pane.autocomplete_down();
                            } else if pane.history_visible {
                                pane.history_next();
                            }
                        }
                        if let Some(ref window) = self.window { window.request_redraw(); }
                        return;
                    }
                    "Enter" => {
                        if let Some(ref mut pane) = self.focused_pane_mut() {
                            if pane.autocomplete_visible {
                                pane.accept_autocomplete();
                            } else if pane.history_visible {
                                pane.accept_history();
                            }
                        }
                        if let Some(ref window) = self.window { window.request_redraw(); }
                        return;
                    }
                    "Escape" => {
                        if let Some(ref mut pane) = self.focused_pane_mut() {
                            if pane.autocomplete_visible {
                                pane.hide_autocomplete();
                            } else if pane.history_visible {
                                pane.hide_history();
                            }
                        }
                        if let Some(ref window) = self.window { window.request_redraw(); }
                        return;
                    }
                    _ => {}
                }
            }
        }

        // ── Ctrl+Space: Show autocomplete popup ──
        if ctrl && !alt && !shift && key == " " {
            if let Some(ref mut pane) = self.focused_pane_mut() {
                pane.show_autocomplete();
            }
            if let Some(ref window) = self.window { window.request_redraw(); }
            return;
        }

        // ── Ctrl+R: Show history search popup ──
        if ctrl && !alt && !shift && key == "r" {
            if let Some(ref mut pane) = self.focused_pane_mut() {
                if pane.history_visible {
                    pane.hide_history();
                } else {
                    pane.show_history();
                }
            }
            if let Some(ref window) = self.window { window.request_redraw(); }
            return;
        }

        // ── Ctrl+/: Dismiss command correction suggestion ──
        if ctrl && !alt && !shift && key == "/" {
            if let Some(ref mut pane) = self.focused_pane_mut() {
                pane.dismiss_correction();
            }
            if let Some(ref window) = self.window { window.request_redraw(); }
            return;
        }

        // Record command in history when Enter is pressed (for future history popup use)
        if key == "Enter" {
            if let Some(ref mut pane) = self.focused_pane_mut() {
                let line = pane.current_line_text();
                pane.record_command(&line);
            }
        }

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

        // 缩放快捷键：Ctrl/Cmd + =/-/0
        let is_zoom_in = key == "=" && ctrl && !alt && !shift && !super_key;
        let is_zoom_out = key == "-" && ctrl && !alt && !shift && !super_key;
        let is_zoom_reset = key == "0" && ctrl && !alt && !shift && !super_key;
        if is_zoom_in || is_zoom_out || is_zoom_reset {
            if let Some(ref mut pane) = self.focused_pane_mut() {
                if is_zoom_in { pane.zoom_in(); }
                else if is_zoom_out { pane.zoom_out(); }
                else { pane.zoom_reset(); }
            }
            if let Some(ref window) = self.window { window.request_redraw(); }
            return;
        }

        if let Some(ref mut pane) = self.focused_pane_mut() {
            // 在 macOS 上，Command 键组合是应用级快捷键，
            // 不要将无法识别的组合转发到终端。
            if cfg!(target_os = "macos") && super_key {
                return;
            }
            pane.handle_key(key, ctrl, alt, shift);
        }
        // 重置光标闪烁定时器（用户输入时让光标显示）
        if let Some(ref mut renderer) = self.renderer {
            renderer.reset_cursor_blink();
        }
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}
