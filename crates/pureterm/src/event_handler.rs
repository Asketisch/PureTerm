use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use pureterm_core::config::ConfigService;
use pureterm_core::theme::ColorScheme;
use pureterm_gui::EguiBackend;
use pureterm_gui::TerminalRenderer;
use pureterm_gui::renderer::ViewportRect;
use pureterm_gui::transparency::TransparencyConfig;
use pureterm_ui::apply_theme;
use pureterm_ui::split::{Direction, PaneRect,
    compute_divider_rects, divider_at_point, pane_at_point};
use pureterm_ui::tabs::TabContent;
use pureterm_ui::terminal::pane::{PaneAction, Selection};
use pureterm_terminal::mark::MarkKind;
use pureterm_terminal::screen::selection::{TerminalMouseEvent, MouseButton, MouseModifiers};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent, MouseButton as WinitMouseButton, MouseScrollDelta};
use winit::event_loop::ActiveEventLoop;

use crate::app::{PureTermApp, RenameModalState, ColorModalState, DividerDrag};
use crate::overlay::parse_hex_color;
use crate::platform;

impl ApplicationHandler for PureTermApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let mut window_attrs = winit::window::Window::default_attributes()
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
            // font_size 配置单位为磅(pt)，fontdue 需要像素(px)
            let scale = window.scale_factor() as f32;
            let font_size = terminal_config.font_size as f32 * scale;
            let renderer = pollster::block_on(async {
                TerminalRenderer::new(terminal_config.font_path.as_deref(), font_size).await
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
            egui_backend.setup_cjk_fonts();

            // 将 PureTerm 主题应用到 egui 上下文（notedeck 启发的设计系统）
            apply_theme(&egui_backend.context, &config.get().appearance.ui_theme);

            let (cols, rows) = renderer.pixel_to_cells(size.width, size.height);

            // 创建初始面板
            let pane_id = self.create_pane(cols, rows);

            // 创建应用并打开初始标签页（使用已加载的配置）
            let mut app = pureterm_ui::app::App::new(config.get().clone());
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
            // 设置 VoiceOver 辅助功能
            #[cfg(target_os = "macos")]
            {
                crate::accessibility::platform::setup_accessibility(self.window.as_ref().unwrap());
            }
            // Enable IME for CJK input methods
            #[cfg(target_os = "macos")]
            {
                self.window.as_ref().unwrap().set_ime_allowed(true);
            }
            // 计算红绿灯区域实际宽度（物理像素）
            #[cfg(target_os = "macos")]
            {
                self.traffic_light_w = crate::platform::macos::traffic_light_width(self.window.as_ref().unwrap());
            }
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
                    || self.rename_modal.is_some() || self.color_modal.is_some()
                    || self.quit_requested =>
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
            WindowEvent::Focused(focused) => {
                // Forward focus events to child processes (DEC mode 1004)
                for pane in self.panes.values_mut() {
                    if pane.terminal.should_report_focus() {
                        let seq = if focused {
                            pureterm_terminal::Terminal::focus_in_sequence()
                        } else {
                            pureterm_terminal::Terminal::focus_out_sequence()
                        };
                        pane.write_input(seq);
                    }
                }
            }
            WindowEvent::CloseRequested => {
                // Check if any pane has a running session and confirm_quit is enabled
                let confirm_quit = self.app.as_ref().map(|a| a.config.confirm_quit).unwrap_or(false);
                let has_active = self.panes.values().any(|p| p.session_is_active());
                if confirm_quit && has_active {
                    self.quit_requested = true;
                    if let Some(ref window) = self.window {
                        window.request_redraw();
                    }
                } else {
                    log::info!("Window closed");
                    event_loop.exit();
                }
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

                // Process pending OSC 133 mark events for prevent-sleep.
                // Mark callbacks are invoked synchronously during drain_output() above,
                // so we drain the shared buffer here on the main thread.
                if let Ok(mut pending) = self.pending_marks.lock() {
                    for (_pane_id, kind) in pending.drain(..) {
                        match kind {
                            MarkKind::CommandStart => {
                                log::debug!("PreventSleep: command started, preventing sleep");
                                self.prevent_sleep.prevent();
                                self.sleep_prevented = true;
                                self.tab_bar.tab_bar_dirty = true;
                            }
                            MarkKind::OutputEnd => {
                                log::debug!("PreventSleep: command output ended, allowing sleep");
                                self.prevent_sleep.allow();
                                self.sleep_prevented = false;
                                self.tab_bar.tab_bar_dirty = true;
                            }
                            _ => {}
                        }
                    }
                }

                // 检查设置是否在外部改变（如 egui 设置面板中），如果是则标记所有面板为脏
                if let Some(ref app) = self.app {
                    let appearance_changed = app.config.appearance != self.appearance;
                    let theme_changed = app.config.appearance.theme != self.color_scheme.name;
                    if appearance_changed || theme_changed {
                        // 更新外观缓存
                        self.appearance = app.config.appearance.clone();
                        // 更新颜色方案
                        if theme_changed {
                            if let Some(scheme) = self.theme_service.get_scheme(&app.config.appearance.theme) {
                                self.color_scheme = scheme.clone();
                            }
                        }
                        // 标记所有面板为脏以触发使用新设置重新渲染
                        for pane in self.panes.values_mut() {
                            pane.mark_dirty();
                        }
                        self.tab_bar.tab_bar_dirty = true;
                    }
                }

                let any_dirty = self.panes.values().any(|p| p.is_dirty());
                log::debug!("RedrawRequested: any_dirty={}, context_menu={}, settings={}, profile={}, welcome={}, rename={}, color={}, quit={}",
                    any_dirty, self.context_menu.visible, self.settings_panel.visible,
                    self.profile_selector.visible, self.welcome_page.visible,
                    self.rename_modal.is_some(), self.color_modal.is_some(), self.quit_requested);
                if !any_dirty && !self.tab_bar.tab_bar_dirty && !self.context_menu.visible && !self.settings_panel.visible && !self.profile_selector.visible && !self.welcome_page.visible && self.rename_modal.is_none() && self.color_modal.is_none() && !self.quit_requested {
                    log::debug!("RedrawRequested: 由于没有脏面板且所有面板都隐藏，提前返回");
                    return;
                }

                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);
                let bg = ColorScheme::parse_color(&self.color_scheme.background);

                // 渲染前更新标签栏布局
                self.update_tab_bar_layout();

                // 在借用渲染器之前收集快照和选择状态
                let mut snapshots: Vec<(u64, ViewportRect, pureterm_terminal::ScreenSnapshot, Option<Selection>, usize, usize)> = Vec::new();
                for (pane_id, rect) in &layout {
                    if let Some(pane) = self.panes.get_mut(pane_id) {
                        let viewport = ViewportRect {
                            x: rect.x, y: rect.y,
                            w: rect.w, h: rect.h,
                        };
                        let mut snapshot = pane.terminal.snapshot();
                        // Attach IME preedit text from the pane
                        snapshot.ime_preedit = pane.ime_preedit.take();
                        let selection = pane.selection.clone();
                        let scroll_offset = pane.terminal.scroll_offset();
                        let history = pane.terminal.history();
                        log::debug!("Render pane {}: viewport=({},{},{},{}) grid=({}x{}) cursor=({},{}) visible={}",
                            pane_id, rect.x, rect.y, rect.w, rect.h,
                            snapshot.rows.len(), snapshot.rows.first().map(|r| r.len()).unwrap_or(0),
                            snapshot.cursor_row, snapshot.cursor_col, snapshot.cursor_visible);
                        pane.clear_dirty();
                        snapshots.push((*pane_id, viewport, snapshot, selection, scroll_offset, history));
                    } else {
                        log::warn!("Layout contains pane {} but not found in panes map!", pane_id);
                    }
                }

                // 收集分隔符矩形
                let tab_bar_offset_y = if self.show_tab_bar() { self.tab_bar.height } else { 0.0 };
                let dividers = self.split_root.as_ref()
                    .map(|root| compute_divider_rects(root, 0.0, tab_bar_offset_y, screen_w, screen_h - tab_bar_offset_y))
                    .unwrap_or_default();

                // 收集焦点边框信息
                let focus_rect = self.focused_pane_id
                    .and_then(|fid| layout.iter().find(|(id, _)| *id == fid).map(|(_, r)| *r));

                // Snapshot popup data before borrowing renderer (avoids borrow conflicts in egui closure)
                let popup_snapshot: Option<(bool, Vec<(String, String, u8)>, usize, bool, Option<String>, Vec<String>, Option<(String, i32, Option<String>)>)> =
                    self.focused_pane().map(|pane| {
                        let ac_vis = pane.autocomplete_visible && !pane.autocomplete_results.is_empty();
                        let ac_res: Vec<(String, String, u8)> = pane.autocomplete_results.iter()
                            .map(|c| (c.text.clone(), c.label.clone(), match c.kind {
                                pureterm_ui::autocomplete::CompletionKind::Command => 0,
                                pureterm_ui::autocomplete::CompletionKind::File => 1,
                                pureterm_ui::autocomplete::CompletionKind::Directory => 2,
                                _ => 3,
                            })).collect();
                        let ac_sel = pane.autocomplete_selected;
                        let hv = pane.history_visible;
                        let hs: Option<String> = pane.command_history.selected_entry().map(|s| s.to_string());
                        let he: Vec<String> = pane.command_history.entries().iter().take(20).cloned().collect();
                        let cd = pane.correction_suggestion.as_ref().filter(|c| c.visible).map(|c| (c.command.clone(), c.exit_code, c.suggestion.clone()));
                        (ac_vis, ac_res, ac_sel, hv, hs, he, cd)
                    });
                let screen_w_for_sleep = self.screen_size().0;
                let popup_visible = self.focused_pane().map_or(false, |p| {
                    p.autocomplete_visible || p.history_visible
                        || p.correction_suggestion.as_ref().map_or(false, |c| c.visible)
                });
                // Popup action collectors (written inside egui closure, applied after renderer borrow ends)
                let mut popup_ac_accepted: Option<String> = None;
                let mut popup_ac_close = false;
                let mut popup_history_close = false;
                let mut popup_correction_close = false;

                if let (Some(ref mut renderer), Some(ref surface)) = (&mut self.renderer, &self.surface) {
                    let color_scheme = self.color_scheme.clone();

                    // 仅在内容变化时重建顶点（光标闪烁等不需要重建）
                    if any_dirty {
                        let mut all_vertices = Vec::new();
                        let mut all_indices = Vec::new();

                        // 渲染每个面板快照
                        log::debug!("Rendering {} snapshots", snapshots.len());
                        for (_pane_id, viewport, snapshot, selection, scroll_offset, history) in &snapshots {
                            let offset = all_vertices.len() as u32;
                            let sel = selection.clone();
                            let sel_cb: Option<Box<dyn Fn(usize, usize) -> bool>> = sel.map(|s| {
                                Box::new(move |row: usize, col: usize| s.contains(row, col))
                                    as Box<dyn Fn(usize, usize) -> bool>
                            });
                            let (verts, idx) = renderer.build_vertices(
                                snapshot, &color_scheme, sel_cb.as_ref().map(|f| f.as_ref()), Some(*viewport),
                                0.0,
                            );
                            log::debug!("  pane {}: verts={}, indices={}", _pane_id, verts.len(), idx.len());
                            all_vertices.extend(verts);
                            all_indices.extend(idx.iter().map(|i| i + offset));

                            // 渲染滚动条（如果有历史记录）
                            if *history > 0 {
                                renderer.build_scrollbar_vertices(
                                    *viewport,
                                    *scroll_offset,
                                    *history,
                                    snapshot.rows.len(),
                                    8.0,
                                    0.2,
                                    &mut all_vertices,
                                    &mut all_indices,
                                );
                            }

                            // 渲染命令块视觉标记
                            if !snapshot.block_markers.is_empty() {
                                renderer.build_block_marker_vertices(
                                    *viewport,
                                    &snapshot.block_markers,
                                    &mut all_vertices,
                                    &mut all_indices,
                                );
                            }

                            // 渲染 IME 预编辑文本
                            if let Some(ref preedit) = snapshot.ime_preedit {
                                if !preedit.is_empty() {
                                    renderer.build_ime_preedit_vertices(
                                        *viewport,
                                        &snapshot,
                                        preedit,
                                        &color_scheme,
                                        &mut all_vertices,
                                        &mut all_indices,
                                    );
                                }
                            }
                        }

                        // 渲染分隔符
                        for div in &dividers {
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

                        // 防止休眠状态指示器 — 右上角小圆点
                        if self.sleep_prevented {
                            let s = self.tab_bar.scale;
                            let sw = screen_w_for_sleep;
                            let cx = sw - 10.0 * s;
                            let cy = self.tab_bar.item_y + self.tab_bar.item_h / 2.0;
                            renderer.build_circle_vertices(
                                cx, cy,
                                3.0 * s,
                                [0.4, 0.85, 0.4, 0.8],
                                &mut all_vertices, &mut all_indices,
                            );
                        }
                        self.tab_bar.tab_bar_dirty = false;

                        // 渲染上下文菜单覆盖层
                        Self::render_context_menu_overlay(&self.context_menu, renderer, &mut all_vertices, &mut all_indices);

                        // 缓存顶点
                        renderer.cache_vertices(all_vertices, all_indices);

                        // 更新 VoiceOver 辅助功能文本（每次脏刷新时更新）
                        #[cfg(target_os = "macos")]
                        if let Some(ref window) = self.window {
                            if let Some((_, _, snapshot, _, _, _)) = snapshots.first() {
                                let text: String = snapshot.rows.iter()
                                    .map(|row| {
                                        row.iter().map(|c| c.c).collect::<String>()
                                    })
                                    .collect::<Vec<String>>()
                                    .join("\n");
                                crate::accessibility::platform::update_accessibility_value(
                                    window, &text,
                                );
                            }
                        }
                    }

                    // 使用缓存顶点绘制
                    let cached_v = renderer.cached_vertices().to_vec();
                    let cached_i = renderer.cached_indices().to_vec();
                    let frame = renderer.draw_to_surface(surface, &cached_v, &cached_i, bg);
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
                        || self.rename_modal.is_some() || self.color_modal.is_some()
                        || self.quit_requested || popup_visible;
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
                            let mut profile_connect_request: Option<pureterm_core::profile::Profile> = None;
                            let rename_modal_rc = self.rename_modal.take().map(|m| Rc::new(RefCell::new(m)));
                            let color_modal_rc = self.color_modal.take().map(|m| Rc::new(RefCell::new(m)));

                            let raw_input = egui_backend.winit_state.take_egui_input(self.window.as_ref().unwrap());
                            let mut rename_done = false;
                            let mut color_done = false;
                            let mut settings_changed = false;
                            #[allow(deprecated)]
                            let full_output = egui_backend.run(raw_input, |ctx| {
                                // 设置面板
                                if settings_visible {
#[allow(deprecated)]
                                    egui::CentralPanel::default().show(ctx, |ui| {
                                        ui.vertical_centered(|ui| {
                                            ui.set_max_width(600.0);
                                            settings_changed = self.settings_panel.show(ui, &mut app.config) || settings_changed;
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

                                // 退出确认对话框
                                if self.quit_requested {
                                    let mut do_quit = false;
                                    let mut do_cancel = false;
                                    egui::Window::new("Quit PureTerm?")
                                        .collapsible(false)
                                        .resizable(false)
                                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                                        .show(ctx, |ui| {
                                            ui.vertical_centered(|ui| {
                                                ui.label("There are running processes in the terminal.");
                                                ui.label("Are you sure you want to quit?");
                                                ui.add_space(12.0);
                                                ui.horizontal(|ui| {
                                                    if ui.button("Quit").clicked() {
                                                        do_quit = true;
                                                    }
                                                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                                        do_cancel = true;
                                                    }
                                                });
                                            });
                                        });
                                    if do_quit {
                                        self.quit_requested = false;
                                        self.should_quit = true;
                                    } else if do_cancel {
                                        self.quit_requested = false;
                                    }
                                }

                                // ── Autocomplete / History / Correction popups (using pre-snapshot data) ──
                                {
                                    if let Some((ac_vis, ac_res, ac_sel, hv, hs, he, cd)) = &popup_snapshot {
                                        if *ac_vis && !ac_res.is_empty() {
                                            egui::Window::new("Autocomplete")
                                                .collapsible(false).resizable(false).title_bar(false)
                                                .anchor(egui::Align2::LEFT_BOTTOM, egui::Vec2::new(50.0, -50.0))
                                                .show(ctx, |ui| {
                                                    ui.set_max_width(350.0); ui.set_max_height(200.0);
                                                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                                        for (i, (text, label, kind)) in ac_res.iter().enumerate() {
                                                            let icon = match *kind { 0 => "> ", 2 => "/ ", _ => "  " };
                                                            if ui.selectable_label(i == *ac_sel, format!("{}{}", icon, label)).clicked() {
                                                                popup_ac_accepted = Some(text.clone());
                                                            }
                                                        }
                                                    });
                                                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) { popup_ac_close = true; }
                                                });
                                        }

                                        if *hv {
                                            egui::Window::new("History")
                                                .collapsible(false).resizable(false).title_bar(false)
                                                .anchor(egui::Align2::LEFT_BOTTOM, egui::Vec2::new(50.0, -50.0))
                                                .show(ctx, |ui| {
                                                    ui.set_max_width(400.0); ui.set_max_height(250.0);
                                                    ui.label(egui::RichText::new("Command History").small().weak());
                                                    ui.separator();
                                                    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                        if let Some(ref sel) = hs {
                                                            ui.colored_label(egui::Color32::LIGHT_BLUE, sel);
                                                            ui.separator();
                                                        }
                                                        for entry in he {
                                                            ui.selectable_label(false, egui::RichText::new(entry).monospace().small());
                                                        }
                                                    });
                                                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) { popup_history_close = true; }
                                                });
                                        }

                                        if let Some((cmd, exit_code, suggestion)) = cd {
                                            egui::Window::new("Command Suggestion")
                                                .collapsible(false).resizable(false).title_bar(false)
                                                .anchor(egui::Align2::LEFT_BOTTOM, egui::Vec2::new(50.0, -80.0))
                                                .show(ctx, |ui| {
                                                    ui.set_max_width(450.0);
                                                    ui.colored_label(egui::Color32::from_rgb(255, 150, 100),
                                                        format!("Command '{}' exited with code {}", cmd, exit_code));
                                                    if let Some(ref sug) = suggestion {
                                                        ui.add_space(4.0); ui.label("Suggestion:");
                                                        ui.colored_label(egui::Color32::LIGHT_GREEN, egui::RichText::new(sug).monospace());
                                                    } else {
                                                        ui.add_space(4.0);
                                                        ui.label(egui::RichText::new("No AI correction available (configure AI in settings)").small().weak());
                                                    }
                                                    ui.add_space(4.0);
                                                    if ui.button("Dismiss").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) { popup_correction_close = true; }
                                                });
                                        }
                                    }
                                }
                            });

                            // 应用模态框的结果
                            // 应用重命名模态框结果（仅在点击确定时设置）
                            if let Some(rename_state) = self.rename_modal.take() {
                                app.tab_manager.set_title(rename_state.tab_id, rename_state.new_title);
                                self.tab_bar.tab_bar_dirty = true;
                                if let Some(ref window) = self.window { window.request_redraw(); }
                            }
                            // 应用颜色模态框结果（仅在点击确定时设置）
                            if let Some(color_state) = self.color_modal.take() {
                                app.tab_manager.set_color(color_state.tab_id, Some(color_state.color));
                                self.tab_bar.tab_bar_dirty = true;
                                if let Some(ref window) = self.window { window.request_redraw(); }
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
                // Apply popup actions after renderer borrow has ended
                if let Some(text) = popup_ac_accepted {
                    if let Some(pane) = self.focused_pane_mut() {
                        pane.write_input(text.as_bytes());
                        pane.autocomplete_visible = false;
                    }
                }
                if popup_ac_close {
                    if let Some(pane) = self.focused_pane_mut() {
                        pane.autocomplete_visible = false;
                    }
                }
                if popup_history_close {
                    if let Some(pane) = self.focused_pane_mut() {
                        pane.history_visible = false;
                    }
                }
                if popup_correction_close {
                    if let Some(pane) = self.focused_pane_mut() {
                        pane.correction_suggestion = None;
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

                    // 检查关闭按钮是否悬停
                    let hovered_close_idx = hovered_idx.and_then(|idx| {
                        let layout = &self.tab_bar.tab_layouts[idx];
                        layout.close_button_rect.as_ref().and_then(|cr| {
                            if cr.contains(self.mouse_pos.0, self.mouse_pos.1) {
                                Some(idx)
                            } else {
                                None
                            }
                        })
                    });

                    let changed = hovered_idx != self.tab_bar.hovered_tab_index
                        || hovered_new_tab != self.tab_bar.hovered_new_tab
                        || hovered_close_idx != self.tab_bar.hovered_close_tab_index;
                    if changed {
                        self.tab_bar.hovered_tab_index = hovered_idx;
                        self.tab_bar.hovered_new_tab = hovered_new_tab;
                        self.tab_bar.hovered_close_tab_index = hovered_close_idx;
                        self.tab_bar.tab_bar_dirty = true;
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
                    let (screen_w, screen_h) = self.screen_size();
                    let mouse_layout = self.compute_layout(screen_w, screen_h);
                    let icon = if let Some(ref info) = self.hovered_divider {
                        match info.direction {
                            Direction::Vertical => winit::window::CursorIcon::ColResize,
                            Direction::Horizontal => winit::window::CursorIcon::RowResize,
                        }
                    } else if let Some(target_id) = pane_at_point(&mouse_layout, self.mouse_pos.0, self.mouse_pos.1) {
                        let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &mouse_layout);
                        if let Some(pane) = self.panes.get(&target_id) {
                            if pane.url_at(row, col).is_some() {
                                winit::window::CursorIcon::Pointer
                            } else {
                                winit::window::CursorIcon::Text
                            }
                        } else {
                            winit::window::CursorIcon::Text
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
                                    if self.mouse_pos.0 < self.traffic_light_w && self.mouse_pos.1 < self.tab_bar.height {
                                        return;
                                    }

                                    if let Some(idx) = self.tab_bar.tab_layouts.iter().position(|l| l.rect.contains(self.mouse_pos.0, self.mouse_pos.1)) {
                                        let layout = &self.tab_bar.tab_layouts[idx];
                                        let tab_id = layout.tab_id;

                                        // 检查是否点击了关闭按钮
                                        if let Some(ref close_rect) = layout.close_button_rect {
                                            if close_rect.contains(self.mouse_pos.0, self.mouse_pos.1) {
                                                // 关闭此标签页（含面板清理）
                                                self.close_tab_by_id(tab_id);
                                                return;
                                            }
                                        }

                                        // 检测双击标签标题 — 打开重命名对话框
                                        let now = std::time::Instant::now();
                                        let is_double_click = self.last_tab_click_id == Some(tab_id)
                                            && self.last_tab_click_time.map_or(false, |t| now.duration_since(t).as_millis() < 500);
                                        self.last_tab_click_time = Some(now);
                                        self.last_tab_click_id = Some(tab_id);

                                        if is_double_click {
                                            // 双击标签页 → 打开重命名对话框
                                            if let Some(ref app) = self.app {
                                                if let Some(tab) = app.tab_manager.active_tab() {
                                                    self.rename_modal = Some(RenameModalState {
                                                        tab_id: tab.id,
                                                        new_title: tab.title.clone(),
                                                    });
                                                }
                                            }
                                            if let Some(ref window) = self.window {
                                                window.request_redraw();
                                            }
                                            return;
                                        }

                                        // 切换到此标签页
                                        let current_active = self.app.as_ref().and_then(|a| a.tab_manager.active_tab()).map(|t| t.id);
                                        if current_active != Some(tab_id) {
                                            self.tab_bar.tab_bar_dirty = true;
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

            // ── IME 事件（CJK 输入法） ──

            WindowEvent::Ime(ime_event) => {
                use winit::event::Ime;
                match ime_event {
                    Ime::Preedit(text, _cursor_pos) => {
                        log::debug!("IME Preedit: {:?}", text);
                        if let Some(ref mut pane) = self.focused_pane_mut() {
                            if text.is_empty() {
                                pane.set_ime_preedit(None);
                            } else {
                                pane.set_ime_preedit(Some(text.to_string()));
                            }
                        }
                        if let Some(ref window) = self.window {
                            window.request_redraw();
                        }
                    }
                    Ime::Commit(text) => {
                        log::debug!("IME Commit: {:?}", text);
                        if let Some(ref mut pane) = self.focused_pane_mut() {
                            pane.commit_ime_text(&text);
                        }
                        if let Some(ref window) = self.window {
                            window.request_redraw();
                        }
                    }
                    Ime::Enabled => {
                        log::debug!("IME Enabled");
                    }
                    Ime::Disabled => {
                        log::debug!("IME Disabled");
                        if let Some(ref mut pane) = self.focused_pane_mut() {
                            pane.set_ime_preedit(None);
                        }
                    }
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // 如果 should_quit 被设置，立即退出事件循环
        if self.should_quit {
            log::info!("Window closed (quit confirmed)");
            _event_loop.exit();
            return;
        }

        // 不要无条件地在这里请求重绘 - 这会导致 RedrawRequested 事件
        // 堆积并阻塞其他事件（如鼠标点击）的处理。
        // 相反，只在实际情况发生变化时才请求重绘。

        // 光标闪烁：如果有面板显示闪烁光标，定期请求重绘
        if let Some(ref renderer) = self.renderer {
            if renderer.is_cursor_blink_enabled() {
                let any_cursor_visible = self.panes.values().any(|p| p.terminal.cursor_visible());
                if any_cursor_visible {
                    if let Some(ref window) = self.window {
                        window.request_redraw();
                    }
                }
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // 被 PTY 输出线程唤醒 — 请求重绘以显示新的终端内容。
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
