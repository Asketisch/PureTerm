use pureterm_ui::terminal::pane::Selection;

use crate::app::PureTermApp;

impl PureTermApp {
    /// 处理键盘输入 - 路由到聚焦面板。
    pub(crate) fn handle_key_input(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool, super_key: bool) {
        log::debug!("handle_key_input: key={:?}, ctrl={}, alt={}, shift={}, super={}", key, ctrl, alt, shift, super_key);

        // ── Copy mode ──────────────────────────────────────────────────────
        // Ctrl+Shift+Space: enter copy mode.
        if ctrl && shift && !alt && !super_key && key == " " {
            if let Some(pane) = self.focused_pane_mut() {
                let (cols, rows) = (pane.terminal.cols(), pane.terminal.rows());
                pane.copy_mode.enter(rows, cols);
                pane.mark_dirty();
            }
            if let Some(ref window) = self.window { window.request_redraw(); }
            return;
        }

        // While copy_mode is active, the focused pane consumes all keys.
        let copy_mode_active = self.focused_pane().map_or(false, |p| p.copy_mode.active);
        if copy_mode_active {
            self.handle_copy_mode_key(key, ctrl, alt, shift);
            if let Some(ref window) = self.window { window.request_redraw(); }
            return;
        }

        // ── Broadcast toggle ──────────────────────────────────────────────
        // Ctrl+Shift+B: add focused pane to broadcast group; second press removes.
        if ctrl && shift && !alt && !super_key && (key == "B" || key == "b") {
            if let Some(id) = self.focused_pane_id {
                let now_member = self.broadcast.toggle(id);
                self.broadcast.enable(self.broadcast.len() >= 2);
                log::info!(
                    "broadcast: pane {} {} group; group size {}; enabled={}",
                    id,
                    if now_member { "joined" } else { "left" },
                    self.broadcast.len(),
                    self.broadcast.is_enabled(),
                );
            }
            if let Some(ref window) = self.window { window.request_redraw(); }
            return;
        }

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

        let mut delivered_to: Option<u64> = None;
        if let Some(focused_id) = self.focused_pane_id {
            if let Some(pane) = self.panes.get_mut(&focused_id) {
                // 在 macOS 上，Command 键组合是应用级快捷键，
                // 不要将无法识别的组合转发到终端。
                if cfg!(target_os = "macos") && super_key {
                    return;
                }
                pane.handle_key(key, ctrl, alt, shift);
                delivered_to = Some(focused_id);
            }
        }
        // Broadcast fan-out: deliver the same key event to every other member.
        if self.broadcast.is_enabled() {
            if let Some(focused_id) = delivered_to {
                if self.broadcast.contains(focused_id) {
                    let members: Vec<u64> = self.broadcast.members();
                    for member_id in members {
                        if member_id == focused_id { continue; }
                        if let Some(pane) = self.panes.get_mut(&member_id) {
                            pane.handle_key(key, ctrl, alt, shift);
                        }
                    }
                }
            }
        }
        // 重置光标闪烁定时器（用户输入时让光标显示）
        if let Some(ref mut renderer) = self.renderer {
            renderer.reset_cursor_blink();
        }
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    fn handle_copy_mode_key(&mut self, key: &str, _ctrl: bool, _alt: bool, _shift: bool) {
        let Some(pane) = self.focused_pane_mut() else { return; };
        match key {
            "h" | "Left" => pane.copy_mode.move_left(),
            "j" | "Down" => pane.copy_mode.move_down(),
            "k" | "Up" => pane.copy_mode.move_up(),
            "l" | "Right" => pane.copy_mode.move_right(),
            "w" => pane.copy_mode.move_word_forward(),
            "b" => pane.copy_mode.move_word_back(),
            "0" | "Home" => pane.copy_mode.move_line_start(),
            "$" | "End" => pane.copy_mode.move_line_end(),
            "v" => pane.copy_mode.start_selection_linear(),
            "V" => pane.copy_mode.start_selection_block(),
            "y" | "Enter" => {
                if let Some((anchor, head, _is_block)) = pane.copy_mode.normalised_selection() {
                    let s_row = anchor.row.max(0) as usize;
                    let e_row = head.row.max(0) as usize;
                    let text = pane.terminal.get_text(s_row, anchor.col, e_row, head.col);
                    if let Some(cb) = pane.on_clipboard_set.as_mut() { cb(&text); }
                }
                pane.copy_mode.exit();
            }
            "Escape" | "q" => {
                pane.copy_mode.cancel_selection();
                pane.copy_mode.exit();
            }
            _ => {}
        }
        pane.mark_dirty();
    }
}
