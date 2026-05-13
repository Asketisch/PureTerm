use pureterm_gui::TerminalRenderer;
use pureterm_gui::renderer::ViewportRect;
use pureterm_ui::context_menu::{ContextMenuState, MENU_ITEM_HEIGHT, MENU_SEPARATOR_HEIGHT};
use pureterm_ui::tab_bar::{TabBarState, TabLayout, ViewportRect as TabViewportRect};

use crate::app::PureTermApp;

impl PureTermApp {
    /// 使用渲染器的字形图集渲染上下文菜单覆盖层。
    pub(crate) fn render_context_menu_overlay(
        context_menu: &ContextMenuState,
        renderer: &mut TerminalRenderer,
        vertices: &mut Vec<pureterm_gui::pipeline::Vertex>,
        indices: &mut Vec<u32>,
    ) {
        if !context_menu.visible {
            return;
        }

        // 菜单背景（圆角矩形）
        let (mx, my, mw, mh) = context_menu.menu_rect();
        let menu_radius = 8.0;
        renderer.build_rounded_rect_vertices(
            ViewportRect { x: mx, y: my, w: mw, h: mh },
            [0.18, 0.18, 0.22, 0.95],
            menu_radius,
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

            // 悬停高亮（圆角矩形）
            if context_menu.hovered_index == Some(i) {
                renderer.build_rounded_rect_vertices(
                    ViewportRect { x: mx + 2.0, y: iy, w: mw - 4.0, h: MENU_ITEM_HEIGHT },
                    hover_bg,
                    4.0,
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

    pub(crate) fn render_tab_bar_overlay(
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

        let s = tab_bar.scale;
        let bar_h = tab_bar.bar_h;
        let item_y = tab_bar.item_y;
        let item_h = tab_bar.item_h;
        let close_btn_w = tab_bar.close_btn_w;
        let padding_h = tab_bar.padding_h;

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
                ViewportRect { x: 0.0, y: 0.0, w: screen_w, h: bar_h },
                [bar_bg[0] * dim, bar_bg[1] * dim, bar_bg[2] * dim, 1.0],
                vertices, indices,
            );
        }

        // 底部边框 — 微妙的分隔符
        {
            let border = 0.25;
            renderer.build_rect_vertices(
                ViewportRect { x: 0.0, y: bar_h - s, w: screen_w, h: s },
                [border, border, border + 0.03, 1.0],
                vertices, indices,
            );
        }

        // "+" 新标签按钮
        {
            let btn = tab_bar.new_tab_rect;
            if btn.w > 0.0 {
                // 左侧分隔线
                let sep_color = [0.35, 0.35, 0.4, 1.0];
                renderer.build_rect_vertices(
                    ViewportRect { x: btn.x, y: item_y + 4.0 * s, w: s, h: item_h - 8.0 * s },
                    sep_color,
                    vertices, indices,
                );

                // 悬停背景（圆角矩形）
                if tab_bar.hovered_new_tab {
                    let hover = 0.3;
                    renderer.build_rounded_rect_vertices(
                        ViewportRect { x: btn.x, y: btn.y, w: btn.w, h: btn.h },
                        [hover, hover, hover + 0.02, 1.0],
                        4.0 * s,
                        vertices, indices,
                    );
                }
                // "+" 符号居中
                let glyph_h = 16.0 * s;
                let plus_y = item_y + (item_h - glyph_h) / 2.0;
                let plus_color = if tab_bar.hovered_new_tab {
                    [1.0f32, 1.0, 1.0, 1.0]
                } else {
                    [0.55f32, 0.55, 0.6, 1.0]
                };
                let plus_w = renderer.measure_text("+");
                renderer.build_text_vertices(
                    "+",
                    btn.x + (btn.w - plus_w) / 2.0,
                    plus_y,
                    plus_color,
                    vertices, indices,
                );
            }
        }

        // 标签页
        let glyph_h = 16.0 * s;
        let text_y = item_y + (item_h - glyph_h) / 2.0;

        for (i, layout) in tab_bar.tab_layouts.iter().enumerate() {
            let is_active = active_tab_id == Some(layout.tab_id);
            let is_hovered = tab_bar.hovered_tab_index == Some(i);

            // 标签页背景（圆角矩形）
            let tab_radius = 6.0 * s;
            if is_active {
                renderer.build_rounded_rect_vertices(
                    ViewportRect { x: layout.rect.x, y: layout.rect.y, w: layout.rect.w, h: layout.rect.h },
                    bar_bg,
                    tab_radius,
                    vertices, indices,
                );
            } else if is_hovered {
                let f = 0.92;
                renderer.build_rounded_rect_vertices(
                    ViewportRect { x: layout.rect.x, y: layout.rect.y, w: layout.rect.w, h: layout.rect.h },
                    [bar_bg[0] * f + 0.05, bar_bg[1] * f + 0.05, bar_bg[2] * f + 0.05, 1.0],
                    tab_radius,
                    vertices, indices,
                );
            }

            // 标题文本 — 避让左侧关闭按钮
            let has_close = layout.close_button_rect.is_some();
            let left_reserved = if has_close { close_btn_w + 4.0 * s } else { padding_h / 2.0 };
            let right_reserved = padding_h / 2.0;
            let available_width = layout.rect.w - left_reserved - right_reserved;
            let full_width = renderer.measure_text(&layout.title);

            let (display_title, text_width) = if full_width <= available_width {
                (layout.title.clone(), full_width)
            } else {
                let mut byte_end = 0usize;
                let mut w = 0.0f32;
                for (i, ch) in layout.title.char_indices() {
                    let cw = renderer.measure_text_char(ch);
                    if w + cw > available_width { break; }
                    w += cw;
                    byte_end = i + ch.len_utf8();
                }
                (layout.title[..byte_end].to_string(), w)
            };

            let title_x = layout.rect.x + left_reserved + (available_width - text_width) / 2.0;
            let fg_color = if is_active {
                [fg_rgb[0] as f32 / 255.0, fg_rgb[1] as f32 / 255.0, fg_rgb[2] as f32 / 255.0, 1.0]
            } else {
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

            // 关闭按钮 — 仅在悬停时可见
            if let Some(ref close_rect) = layout.close_button_rect {
                if is_hovered {
                    let btn_size = 24.0 * s;
                    let btn_y = item_y + (item_h - btn_size) / 2.0;
                    let btn_x = close_rect.x + (close_rect.w - btn_size) / 2.0;
                    let is_close_hovered = tab_bar.hovered_close_tab_index == Some(i);
                    let close_bg = if is_close_hovered {
                        [0.4, 0.15, 0.15, 1.0]
                    } else if is_active {
                        [0.25, 0.25, 0.28, 1.0]
                    } else {
                        [0.18, 0.18, 0.21, 1.0]
                    };
                    let circle_cx = btn_x + btn_size / 2.0;
                    let circle_cy = btn_y + btn_size / 2.0;
                    renderer.build_circle_vertices(
                        circle_cx,
                        circle_cy,
                        btn_size / 2.0,
                        close_bg,
                        vertices, indices,
                    );
                    let x_color = if is_close_hovered {
                        [1.0f32, 0.5, 0.5, 1.0]
                    } else {
                        [0.7f32, 0.45, 0.45, 1.0]
                    };
                    let x_glyph_w = renderer.measure_text("×");
                    renderer.build_text_vertices(
                        "×",
                        btn_x + (btn_size - x_glyph_w) / 2.0,
                        btn_y + (btn_size - glyph_h) / 2.0,
                        x_color,
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
                        y: layout.rect.y + layout.rect.h - 3.0 * s,
                        w: layout.rect.w,
                        h: 3.0 * s,
                    },
                    [cv[0] as f32 / 255.0, cv[1] as f32 / 255.0, cv[2] as f32 / 255.0, 1.0],
                    vertices, indices,
                );
            }
        }
    }
}

/// 将十六进制颜色字符串（如 "#ff0000"）解析为 [u8; 3]。
pub(crate) fn parse_hex_color(s: &str) -> [u8; 3] {
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
