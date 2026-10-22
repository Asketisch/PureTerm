//! iced `shader::Program` 实现:`TerminalProgram`(状态)+ `TerminalState`
//! (空状态)+ `Program::draw`。
//!
//! `draw()` 把终端网格快照转换为 GPU primitive:逐 cell 收集背景 quad、
//! 文本 glyph 顶点、选区/光标/vi 模式/触发器高亮叠加层,并取出字形 atlas 脏
//! 数据,最后构造一个 [`super::primitive::TerminalPrimitive`]。

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use iced::widget::shader;
use iced::Rectangle;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as TermPoint};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

use crate::background_image::{BackgroundMode, DecodedBackground};
use crate::terminal_context::TerminalContext;
use pureterm_alacritty_runtime::renderer::glyph_atlas::AtlasKind;

use super::glyph::GlyphState;
use super::helpers::{bits_to_f32, color_to_f32, push_bg_quad};
use super::image_quad::{ImageQuadDraw, ImageQuadState};
use super::overrides::TermColorOverrides;
use super::pipeline::GpuResources;
use super::primitive::TerminalPrimitive;
use super::vertices::{BgVertex, TextVertex};

pub struct TerminalProgram {
    pub ctx: Arc<TerminalContext>,
    pub glyph_state: Arc<RefCell<GlyphState>>,
    pub background_color: [f32; 4],
    pub foreground_color: [f32; 4],
    pub opacity: f32,
    /// 共享的原子变量，保存*实际*视口缩放因子，由 `prepare()`
    /// 每帧写入。`draw()` 读取它，在值变化时重新配置字形图集。
    pub viewport_scale_factor: Arc<AtomicU32>,
    /// GAP-4.1：共享的 vi 搜索模式，用于匹配高亮。用户在搜索栏输入时
    /// 应用写它；shader 在 `draw()` 期间读取它以高亮匹配。
    pub vi_search_pattern: Arc<Mutex<Option<String>>>,
    /// GAP-3.8/4.3：以模式字符串为键缓存的 `RegexSearch`。没有它，
    /// `draw()` 每帧重建 4 个惰性 DFA（terminal_shader 每帧都会跑
    /// `RegexSearch::new`）。应用持有 `Arc` 并克隆进每个逐帧
    /// `TerminalProgram`；因此缓存跨帧保留。仅在模式字符串变化时重建。
    pub cached_search:
        Arc<Mutex<Option<(String, alacritty_terminal::term::search::RegexSearch)>>>,
    /// GAP-10：活动面板的触发器高亮区域（应用每帧发布到这个共享 `Arc`，
    /// 镜像 `vi_search_pattern`）。
    pub trigger_highlights:
        Arc<Mutex<Vec<crate::triggers::HighlightRegion>>>,
    /// GAP-10：活动面板的已折叠折叠区域。
    pub trigger_folds: Arc<Mutex<Vec<crate::triggers::FoldRegion>>>,

    // GAP-54：共享的背景图片（加载一次，克隆进每个逐帧 primitive）。
    // 签名让 primitive 在无变化时跳过重新上传 GPU 纹理。
    /// GAP-72:预设注入的 ANSI 16 色。`None` = 用内核默认色板
    /// (`helpers::named_color_default` 的回退板)。
    pub ansi_palette: Option<[Rgb; 16]>,
    /// GAP-72:终端内叠加层配色(光标 / 选区 / 搜索命中 / 折叠遮罩 /
    /// vi 光标)。`None` = 用 `TermColorOverrides::default()`(与改动前
    /// 逐值一致)。
    pub term_overrides: Option<TermColorOverrides>,
    pub background_image: Arc<Mutex<Option<DecodedBackground>>>,
    pub background_mode: BackgroundMode,
    pub background_opacity: f32,
    pub background_signature: u64,

    // T-core-graphics v3：图像 quad 通道的共享状态。应用在构造时
    // 构建这些 `Arc` 一次，并把句柄克隆进每个逐帧 `TerminalPrimitive`。
    // 渲染线程随后：
    //   1. 排空解码通道（`decode_rx`），把新解码的 RGBA 像素推进
    //      `image_quad_state.atlas`。
    //   2. 读取 `image_quad_draws` 构建逐帧顶点/索引缓冲。
    //
    // 三者都包在 `Arc<Mutex<_>>` 里，因为 `iced::shader` 要求
    // `Program: Send + Sync + 'static`，且 GPU 渲染线程在 `Primitive::prepare`
    // 中通过 `&self` 借用它们。
    pub image_quad_state: Arc<Mutex<ImageQuadState>>,
    pub image_quad_draws: Arc<Mutex<Vec<ImageQuadDraw>>>,
    pub decode_rx: Arc<Mutex<std::sync::mpsc::Receiver<crate::image_decode_worker::DecodedImage>>>,
}

#[derive(Default)]
pub struct TerminalState;

impl<M> shader::Program<M> for TerminalProgram {
    type State = TerminalState;
    type Primitive = TerminalPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: iced::mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        // 检查渲染线程是否报告了不同的视口缩放因子（由 `prepare()` 在上一帧
        // 写入）。若是，重新配置字形图集以按正确分辨率光栅化。原子变量
        // 初始化为 1.0（而非 0），使第一帧有效。
        let vp_scale = bits_to_f32(self.viewport_scale_factor.load(Ordering::Relaxed));
        if self.glyph_state.borrow_mut().update_scale_factor(vp_scale) {
            log::info!("Retina: reconfigured glyph atlas for scale_factor={}", vp_scale);
        }

        // 所有顶点位置都保持逻辑坐标。iced_wgpu 把 GPU 视口设为
        // bounds × scale_factor，因此顶点着色器把逻辑坐标经 NDC 映射到物理像素。
        let screen_w = bounds.width;
        let screen_h = bounds.height;
        // 把窗口不透明度应用到背景色 alpha 通道
        let bg_color = [
            self.background_color[0],
            self.background_color[1],
            self.background_color[2],
            self.background_color[3] * self.opacity,
        ];

        // 阶段 1：在短锁下快照终端状态
        struct CellSnapshot {
            row: usize,
            col: usize,
            c: char,
            fg: Color,
            bg: Color,
        }

        let (cells, display_offset, cursor_row, cursor_col, colors, selection_range, vi_mode_active, vi_cursor_row, vi_cursor_col) = {
            let term = self.ctx.terminal().lock_unfair();
            let display_offset = term.grid().display_offset();
            let cursor = &term.grid().cursor;
            let cursor_row = (cursor.point.line.0 + display_offset as i32) as usize;
            let cursor_col = cursor.point.column.0;
            let colors = *term.colors();
            let cells: Vec<CellSnapshot> = term.grid().display_iter()
                .map(|indexed| CellSnapshot {
                    row: (indexed.point.line.0 + display_offset as i32) as usize,
                    col: indexed.point.column.0,
                    c: indexed.cell.c,
                    fg: indexed.cell.fg,
                    bg: indexed.cell.bg,
                })
                .collect();
            // 选区高亮:把 Selection 解析为 SelectionRange(可能跨多行/可能为 Block)。
            // 渲染时(下方 cell 循环)用 SelectionRange::contains(point) 判定每格是否高亮。
            let selection_range = term.selection.as_ref().and_then(|s| s.to_range(&*term));
            // GAP-4.1：vi 模式光标信息
            let vi_mode_active = term.mode().contains(TermMode::VI);
            let (vi_cursor_row, vi_cursor_col) = if vi_mode_active {
                let vc = &term.vi_mode_cursor.point;
                let row = (vc.line.0 + display_offset as i32) as usize;
                let col = vc.column.0;
                (row, col)
            } else {
                (0usize, 0usize)
            };
            (cells, display_offset, cursor_row, cursor_col, colors, selection_range, vi_mode_active, vi_cursor_row, vi_cursor_col)
        }; // 锁在这里释放

        // 把配置的前景色注入 colors 数组，使 color_to_f32() 能按配置
        // 解析 NamedColor::Foreground，而不是回退到硬编码默认值。
        let mut colors = colors;
        let fg_rgb = Rgb {
            r: (self.foreground_color[0] * 255.0) as u8,
            g: (self.foreground_color[1] * 255.0) as u8,
            b: (self.foreground_color[2] * 255.0) as u8,
        };
        colors[NamedColor::Foreground as usize] = Some(fg_rgb);

        // GAP-72:预设的 ANSI 16 色注入。`NamedColor::Black..BrightWhite`
        // 的判别值正好是 0..15,与 256 色调色板前 16 项同槽 —— 一次写入
        // 同时覆盖具名色与 `38;5;N` 索引色。
        if let Some(ansi) = self.ansi_palette.as_ref() {
            super::overrides::inject_ansi_palette(&mut colors, ansi);
        }

        // GAP-72:叠加层配色。`None` → 与改动前逐值相同的默认值。
        let ov = self.term_overrides.unwrap_or_default();

        // 阶段 2：从快照构建 GPU primitive（不持锁）
        let mut gs = self.glyph_state.borrow_mut();
        let cell_w = gs.cell_width; // 逻辑像素
        let cell_h = gs.cell_height; // 逻辑像素
        let font_descent = gs.font_descent; // 逻辑像素

        let computed_cols = (screen_w / cell_w).floor().max(1.0) as usize;
        let computed_rows = (screen_h / cell_h).floor().max(1.0) as usize;
        let pad_x = (screen_w - computed_cols as f32 * cell_w).max(0.0) / 2.0;
        let pad_y = (screen_h - computed_rows as f32 * cell_h).max(0.0) / 2.0;

        let mut bg_vertices: Vec<BgVertex> = Vec::new();
        let mut bg_indices: Vec<u32> = Vec::new();
        let mut text_vertices: Vec<TextVertex> = Vec::new();
        let mut text_indices: Vec<u32> = Vec::new();

        // 全屏背景。它是 bg 通道里的**第一个** quad,单独记下它的索引
        // 条数(6):绘制时要先画它,再画背景图,最后画单元格底色 ——
        // 否则不透明的全屏底色会把背景图整个盖住(背景图功能形同
        // 虚设)。
        push_bg_quad(&mut bg_vertices, &mut bg_indices, 0.0, 0.0, screen_w, screen_h, bg_color);
        let bg_base_index_count = bg_indices.len() as u32;

        // 选区高亮颜色。alacritty kernel 的 Colors 数组没有 selection 槽,
        // 色彩配置不通过它走;GAP-72 起由预设决定(classic 保持原蓝色)。
        let sel_color: [f32; 4] = ov.selection;

        // GAP-4.1：vi 搜索匹配高亮。若设置了模式，计算可见的搜索匹配。
        let search_match_color: [f32; 4] = ov.search_hit; // 暖琥珀色
        let search_matches: Vec<std::ops::RangeInclusive<TermPoint>> = {
            let pattern = self.vi_search_pattern.lock().ok().and_then(|p| p.clone());
            match pattern {
                Some(pat) if !pat.is_empty() => {
                    let term = self.ctx.terminal().lock();
                    // 以模式字符串为键缓存构建好的 RegexSearch，避免每帧重建
                    // 4 个惰性 DFA。守卫在借用 regex 的整个生命周期内持有
                    // （RegexIter 需要 `&mut RegexSearch`）。
                    let mut cache = match self.cached_search.lock() {
                        Ok(g) => g,
                        // 中毒的互斥锁——恢复内部守卫，而不是丢掉全部匹配。
                        Err(e) => e.into_inner(),
                    };
                    let stale = cache.as_ref().map_or(true, |(p, _)| *p != pat);
                    if stale {
                        *cache = alacritty_terminal::term::search::RegexSearch::new(&pat)
                            .ok()
                            .map(|r| (pat.clone(), r));
                    }
                    match cache.as_mut() {
                        Some((_, regex)) => {
                            let top = term.topmost_line();
                            let screen = term.screen_lines() as i32;
                            let start = TermPoint::new(top, Column(0));
                            let end = TermPoint::new(
                                Line(-(display_offset as i32) + screen - 1),
                                term.last_column(),
                            );
                            let iter = alacritty_terminal::term::search::RegexIter::new(
                                start,
                                end,
                                alacritty_terminal::index::Direction::Right,
                                &*term,
                                regex,
                            );
                            iter.collect()
                        }
                        None => Vec::new(),
                    }
                }
                _ => Vec::new(),
            }
        };
        // 辅助：判断网格点是否落在任一搜索匹配内
        let is_search_match = |p: TermPoint| -> bool {
            search_matches.iter().any(|m| m.contains(&p))
        };

        // GAP-10：触发器高亮区域（预过滤到可见视口）。
        let trigger_regions: Vec<crate::triggers::HighlightRegion> = {
            let guard = match self.trigger_highlights.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let top = -(display_offset as i32);
            let bottom = -(display_offset as i32) + computed_rows as i32 - 1;
            guard
                .iter()
                .filter(|r| r.line >= top && r.line <= bottom)
                .copied()
                .collect()
        };

        let trigger_folds: Vec<crate::triggers::FoldRegion> = {
            let guard = match self.trigger_folds.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let top = -(display_offset as i32);
            let bottom = -(display_offset as i32) + computed_rows as i32 - 1;
            guard
                .iter()
                .filter(|f| f.collapsed && f.bottom_line >= top && f.top_line <= bottom)
                .copied()
                .collect()
        };

        let fold_conceal_color: [f32; 4] = ov.fold_conceal;

        // 遍历 cell 快照：背景 + 文本字形
        for cell in &cells {
            let x = pad_x + cell.col as f32 * cell_w;
            let y = pad_y + cell.row as f32 * cell_h;

            // cell 背景
            if cell.bg != Color::Named(NamedColor::Background) {
                let color = color_to_f32(cell.bg, &colors);
                push_bg_quad(&mut bg_vertices, &mut bg_indices, x, y, cell_w, cell_h, color);
            }

            // 选区高亮(画在 cell 自身背景之上、glyph 之前)。
            // cell.row 是 display 行号(grid_line + display_offset),
            // selection_range 用 grid Line(可能为负),需换算到同一坐标系再判定。
            if let Some(range) = selection_range {
                let grid_line = Line(cell.row as i32 - display_offset as i32);
                let p = TermPoint::new(grid_line, Column(cell.col));
                if range.contains(p) {
                    push_bg_quad(
                        &mut bg_vertices,
                        &mut bg_indices,
                        x, y, cell_w, cell_h,
                        sel_color,
                    );
                }
            }

            // GAP-4.1：搜索匹配高亮（琥珀色叠加）
            if !search_matches.is_empty() {
                let grid_line = Line(cell.row as i32 - display_offset as i32);
                let p = TermPoint::new(grid_line, Column(cell.col));
                if is_search_match(p) {
                    push_bg_quad(
                        &mut bg_vertices,
                        &mut bg_indices,
                        x, y, cell_w, cell_h,
                        search_match_color,
                    );
                }
            }

            // GAP-10：触发器匹配高亮（按规则着色的叠加层）。
            if !trigger_regions.is_empty() {
                let grid_line = Line(cell.row as i32 - display_offset as i32);
                for region in &trigger_regions {
                    if region.line == grid_line.0
                        && cell.col >= region.col_start
                        && cell.col < region.col_end
                    {
                        push_bg_quad(
                            &mut bg_vertices,
                            &mut bg_indices,
                            x, y, cell_w, cell_h,
                            region.color,
                        );
                        break;
                    }
                }
            }

            let grid_line_i32 = cell.row as i32 - display_offset as i32;
            let fold_concealed = trigger_folds.iter().any(|f| {
                f.collapsed && grid_line_i32 > f.top_line && grid_line_i32 <= f.bottom_line
            });
            if fold_concealed {
                push_bg_quad(
                    &mut bg_vertices,
                    &mut bg_indices,
                    x, y, cell_w, cell_h,
                    fold_conceal_color,
                );
            }

            // 文本字形——跳过被折叠隐藏的内部行。
            if fold_concealed {
                continue;
            }

            if cell.c == ' ' || cell.c == '\t' || cell.c == '\0' {
                continue;
            }

            if let Some(glyph) = gs.get_or_rasterize(cell.c) {
                let fg = color_to_f32(cell.fg, &colors);
                let atlas_sel = if glyph.atlas_kind == AtlasKind::Color { 1.0 } else { 0.0 };
                let glyph_color = if glyph.atlas_kind == AtlasKind::Color {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    fg
                };

                // 字形偏移已是逻辑坐标
                let gx = x + glyph.x_offset;
                let gy = y + (cell_h + font_descent - glyph.y_offset);
                let gw = glyph.width;
                let gh = glyph.height;

                let base = text_vertices.len() as u32;
                text_vertices.extend([
                    TextVertex { position: [gx, gy], tex_coords: [glyph.u, glyph.v], color: glyph_color, atlas_selector: atlas_sel, x_offset: 0.0, y_offset: 0.0 },
                    TextVertex { position: [gx + gw, gy], tex_coords: [glyph.u2, glyph.v], color: glyph_color, atlas_selector: atlas_sel, x_offset: 0.0, y_offset: 0.0 },
                    TextVertex { position: [gx + gw, gy + gh], tex_coords: [glyph.u2, glyph.v2], color: glyph_color, atlas_selector: atlas_sel, x_offset: 0.0, y_offset: 0.0 },
                    TextVertex { position: [gx, gy + gh], tex_coords: [glyph.u, glyph.v2], color: glyph_color, atlas_selector: atlas_sel, x_offset: 0.0, y_offset: 0.0 },
                ]);
                text_indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }

        // 光标
        let cursor_color: [f32; 4] = ov.cursor;
        let cx = pad_x + cursor_col as f32 * cell_w;
        let cy = pad_y + cursor_row as f32 * cell_h;
        push_bg_quad(&mut bg_vertices, &mut bg_indices, cx, cy, cell_w, cell_h, cursor_color);

        // GAP-4.1：vi 模式光标——用青/青色与普通光标视觉区分。
        // vi 模式下主光标变暗，vi 光标突出。
        if vi_mode_active {
            // 调暗主光标
            let dim_color: [f32; 4] = ov.vi_dim;
            push_bg_quad(&mut bg_vertices, &mut bg_indices, cx, cy, cell_w, cell_h, dim_color);
            // 用青色渲染 vi 光标
            let vi_color: [f32; 4] = ov.vi_cursor;
            let vx = pad_x + vi_cursor_col as f32 * cell_w;
            let vy = pad_y + vi_cursor_row as f32 * cell_h;
            push_bg_quad(&mut bg_vertices, &mut bg_indices, vx, vy, cell_w, cell_h, vi_color);
        }

        // 取出图集脏数据（修复 2：用按区域上传而非整份 Vec 克隆）。
        // 返回 Arc<Vec<u8>> + 脏矩形；GPU 侧遍历区域，为每个矩形发出
        // 一次 `queue.write_texture`。
        let grayscale_atlas = gs.atlas.take_dirty_regions();
        let color_atlas = gs.atlas.take_color_dirty_regions();

        // T-core-graphics v3：为图像 quad 通道缓存 cell 尺寸。渲染线程
        // 无法借用 `glyph_state`（它是早先借用的 `RefCell`）；在下方
        // `drop(gs)` 之前先捕获。
        let (cell_w, cell_h) = (gs.cell_width, gs.cell_height);

        drop(gs);

        TerminalPrimitive {
            bg_vertices,
            bg_indices,
            bg_base_index_count,
            text_vertices,
            text_indices,
            grayscale_atlas,
            color_atlas,
            gpu: Mutex::new(GpuResources::default()),
            viewport_scale_factor: self.viewport_scale_factor.clone(),
            background_image: self.background_image.lock().unwrap().clone(),
            background_mode: self.background_mode,
            background_opacity: self.background_opacity,
            background_signature: self.background_signature,
            image_quad_state: self.image_quad_state.clone(),
            image_quad_draws: self.image_quad_draws.clone(),
            decode_rx: self.decode_rx.clone(),
            cell_w,
            cell_h,
        }
    }
}
