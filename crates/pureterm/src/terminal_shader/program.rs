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
use super::pipeline::GpuResources;
use super::primitive::TerminalPrimitive;
use super::vertices::{BgVertex, TextVertex};

pub struct TerminalProgram {
    pub ctx: Arc<TerminalContext>,
    pub glyph_state: Arc<RefCell<GlyphState>>,
    pub background_color: [f32; 4],
    pub foreground_color: [f32; 4],
    pub opacity: f32,
    /// Shared atomic for the *actual* viewport scale factor, written by
    /// `prepare()` each frame. `draw()` reads it and reconfigures the
    /// glyph atlas when the value changes.
    pub viewport_scale_factor: Arc<AtomicU32>,
    /// GAP-4.1: Shared vi search pattern for match highlights.
    /// The app sets this when the user types in the search bar;
    /// the shader reads it during `draw()` to highlight matches.
    pub vi_search_pattern: Arc<Mutex<Option<String>>>,
    /// GAP-3.8/4.3: Cached built `RegexSearch` keyed on the pattern string.
    /// Without this, `draw()` rebuilt 4 lazy DFAs every frame (terminal_shader
    /// ran `RegexSearch::new` per frame). The app owns the `Arc` and clones it
    /// into each per-frame `TerminalProgram`; the cache therefore persists
    /// across frames. Rebuilt only when the pattern string changes.
    pub cached_search:
        Arc<Mutex<Option<(String, alacritty_terminal::term::search::RegexSearch)>>>,
    /// GAP-10: Trigger highlight regions for the active pane (published each frame
    /// by the app into this shared `Arc`, mirroring `vi_search_pattern`).
    pub trigger_highlights:
        Arc<Mutex<Vec<crate::triggers::HighlightRegion>>>,
    /// GAP-10: collapsed fold regions for the active pane.
    pub trigger_folds: Arc<Mutex<Vec<crate::triggers::FoldRegion>>>,

    // GAP-54: shared background image (loaded once, cloned into each per-frame
    // primitive). The signature lets the primitive skip re-uploading the GPU
    // texture when nothing changed.
    pub background_image: Arc<Mutex<Option<DecodedBackground>>>,
    pub background_mode: BackgroundMode,
    pub background_opacity: f32,
    pub background_signature: u64,

    // T-core-graphics v3: shared state for the image quad pass. The app
    // builds these `Arc`s once at construction time and clones the handles
    // into every per-frame `TerminalPrimitive`. The render thread then:
    //   1. Drains the decode channel (`decode_rx`) to push freshly decoded
    //      RGBA pixels into `image_quad_state.atlas`.
    //   2. Reads `image_quad_draws` to build per-frame vertex/index buffers.
    //
    // All three are wrapped in `Arc<Mutex<_>>` because `iced::shader`
    // requires `Program: Send + Sync + 'static` and the GPU render thread
    // borrows them via `&self` inside `Primitive::prepare`.
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
        // Check if the renderer thread has reported a different viewport scale
        // factor (written by `prepare()` on the previous frame). If so,
        // reconfigure the glyph atlas to rasterize at the correct resolution.
        // The atomic is initialized to 1.0 (not 0) so the first frame is valid.
        let vp_scale = bits_to_f32(self.viewport_scale_factor.load(Ordering::Relaxed));
        if self.glyph_state.borrow_mut().update_scale_factor(vp_scale) {
            log::info!("Retina: reconfigured glyph atlas for scale_factor={}", vp_scale);
        }

        // All vertex positions remain in logical coordinates.
        // iced_wgpu sets the GPU viewport to bounds × scale_factor, so the
        // vertex shader maps logical positions through NDC to physical pixels.
        let screen_w = bounds.width;
        let screen_h = bounds.height;
        // Apply window opacity to background color alpha channel
        let bg_color = [
            self.background_color[0],
            self.background_color[1],
            self.background_color[2],
            self.background_color[3] * self.opacity,
        ];

        // Phase 1: Snapshot terminal state under a short lock
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
            // GAP-4.1: Vi mode cursor info
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
        }; // Lock released here

        // Inject configured foreground color into the colors array so that
        // color_to_f32() can resolve NamedColor::Foreground from config
        // instead of falling back to hardcoded defaults.
        let mut colors = colors;
        let fg_rgb = Rgb {
            r: (self.foreground_color[0] * 255.0) as u8,
            g: (self.foreground_color[1] * 255.0) as u8,
            b: (self.foreground_color[2] * 255.0) as u8,
        };
        colors[NamedColor::Foreground as usize] = Some(fg_rgb);

        // Phase 2: Build GPU primitives from snapshot (no lock held)
        let mut gs = self.glyph_state.borrow_mut();
        let cell_w = gs.cell_width;   // logical
        let cell_h = gs.cell_height;  // logical
        let font_descent = gs.font_descent; // logical

        let computed_cols = (screen_w / cell_w).floor().max(1.0) as usize;
        let computed_rows = (screen_h / cell_h).floor().max(1.0) as usize;
        let pad_x = (screen_w - computed_cols as f32 * cell_w).max(0.0) / 2.0;
        let pad_y = (screen_h - computed_rows as f32 * cell_h).max(0.0) / 2.0;

        let mut bg_vertices: Vec<BgVertex> = Vec::new();
        let mut bg_indices: Vec<u32> = Vec::new();
        let mut text_vertices: Vec<TextVertex> = Vec::new();
        let mut text_indices: Vec<u32> = Vec::new();

        // Full-screen background
        push_bg_quad(&mut bg_vertices, &mut bg_indices, 0.0, 0.0, screen_w, screen_h, bg_color);

        // 选区高亮颜色(半透明蓝)。
        // alacritty kernel 的 Colors 数组没有 selection 槽,色彩配置不通过它走;
        // 这里固定一个 macOS 系蓝色,后续可暴露为 config 项。
        let sel_color: [f32; 4] = [0.30, 0.50, 0.90, 0.40];

        // GAP-4.1: Vi search match highlighting.
        // Compute visible search matches if a pattern is set.
        let search_match_color: [f32; 4] = [0.85, 0.65, 0.15, 0.35]; // warm amber
        let search_matches: Vec<std::ops::RangeInclusive<TermPoint>> = {
            let pattern = self.vi_search_pattern.lock().ok().and_then(|p| p.clone());
            match pattern {
                Some(pat) if !pat.is_empty() => {
                    let term = self.ctx.terminal().lock();
                    // Cache the built RegexSearch keyed on the pattern string so
                    // we don't rebuild 4 lazy DFAs every frame. The guard is held
                    // for the lifetime of the borrowed regex (RegexIter needs
                    // `&mut RegexSearch`).
                    let mut cache = match self.cached_search.lock() {
                        Ok(g) => g,
                        // Poisoned mutex — recover the inner guard rather than
                        // dropping matches entirely.
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
        // Helper: check if a grid point falls inside any search match
        let is_search_match = |p: TermPoint| -> bool {
            search_matches.iter().any(|m| m.contains(&p))
        };

        // GAP-10: trigger highlight regions (pre-filtered to the visible viewport).
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

        let fold_conceal_color: [f32; 4] = [0.15, 0.15, 0.18, 0.85];

        // Iterate cell snapshots: backgrounds + text glyphs
        for cell in &cells {
            let x = pad_x + cell.col as f32 * cell_w;
            let y = pad_y + cell.row as f32 * cell_h;

            // Cell background
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

            // GAP-4.1: Search match highlighting (amber overlay)
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

            // GAP-10: Trigger match highlighting (per-rule color overlay).
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

            // Text glyph — skip concealed fold interior rows.
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

                // Glyph offsets are already in logical coordinates
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

        // Cursor
        let cursor_color = [0.6, 0.6, 0.6, 0.8];
        let cx = pad_x + cursor_col as f32 * cell_w;
        let cy = pad_y + cursor_row as f32 * cell_h;
        push_bg_quad(&mut bg_vertices, &mut bg_indices, cx, cy, cell_w, cell_h, cursor_color);

        // GAP-4.1: Vi mode cursor — teal/cyan to visually distinguish from normal cursor.
        // In vi mode the primary cursor is dimmed and the vi cursor is prominent.
        if vi_mode_active {
            // Dim the primary cursor
            let dim_color = [0.4, 0.4, 0.4, 0.3];
            push_bg_quad(&mut bg_vertices, &mut bg_indices, cx, cy, cell_w, cell_h, dim_color);
            // Render vi cursor in teal
            let vi_color = [0.2, 0.8, 0.7, 0.85];
            let vx = pad_x + vi_cursor_col as f32 * cell_w;
            let vy = pad_y + vi_cursor_row as f32 * cell_h;
            push_bg_quad(&mut bg_vertices, &mut bg_indices, vx, vy, cell_w, cell_h, vi_color);
        }

        // Take dirty atlas data (Fix 2: use per-region upload instead of full
        // Vec clone). Returns Arc<Vec<u8>> + dirty rects; the GPU side loops
        // regions and issues one `queue.write_texture` per rect.
        let grayscale_atlas = gs.atlas.take_dirty_regions();
        let color_atlas = gs.atlas.take_color_dirty_regions();

        // T-core-graphics v3: cache cell dimensions for the image quad pass.
        // The render thread can't borrow `glyph_state` (it's a `RefCell`
        // borrowed earlier); capture before `drop(gs)` below.
        let (cell_w, cell_h) = (gs.cell_width, gs.cell_height);

        drop(gs);

        TerminalPrimitive {
            bg_vertices,
            bg_indices,
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
