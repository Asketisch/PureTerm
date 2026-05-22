//! Per-frame vertex array builder for wgpu render passes.
//!
//! Converts `RenderableContent` iterator output into per-pass vertex arrays
//! (background, text, decorations, cursor). The `FrameBuilder` owns reusable
//! `Vec`s to avoid per-frame heap allocation.

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;

use crate::display::color::Rgb;
use crate::display::SizeInfo;
use crate::display::content::{RenderableCell, RenderableContent, RenderableCursor};

use super::glyph_atlas::WgpuGlyphAtlas;
use super::render_pass::ScreenUniforms;

// Re-export vertex types from render_pass modules so all code uses the same types.
pub use super::render_pass::bg_color::BgVertex;
pub use super::render_pass::cell_text::TextVertex;
pub use super::render_pass::decorations::DecoVertex;
pub use super::render_pass::cursor::CursorVertex;

// ---------------------------------------------------------------------------
// FrameBuilder — reusable vertex builders.
// ---------------------------------------------------------------------------

/// Reusable vertex builders that own their `Vec`s (avoids per-frame allocation).
pub struct FrameBuilder {
    /// Full-screen background color quad.
    pub bg_color_verts: Vec<BgVertex>,
    pub bg_color_idxs: Vec<u32>,
    /// Per-cell background quads.
    pub cell_bg_verts: Vec<BgVertex>,
    pub cell_bg_idxs: Vec<u32>,
    /// Text (glyph) quads.
    pub text_verts: Vec<TextVertex>,
    pub text_idxs: Vec<u32>,
    /// Decoration (underline / strikeout) quads.
    pub deco_verts: Vec<DecoVertex>,
    pub deco_idxs: Vec<u32>,
    /// Cursor quads.
    pub cursor_verts: Vec<CursorVertex>,
    pub cursor_idxs: Vec<u32>,
}

/// Result of building a frame's vertex arrays.
pub struct FrameBuildResult {
    /// Screen-space uniforms for the render passes.
    pub screen_uniforms: ScreenUniforms,
    /// The terminal's default background color as `[R, G, B, A]` floats.
    pub bg_color: [f32; 4],
    /// Whether the frame needs a full redraw.
    pub full_redraw: bool,
}

impl FrameBuilder {
    /// Create a new `FrameBuilder` with pre-allocated capacity.
    pub fn new() -> Self {
        let cell_cap = 8192;
        Self {
            bg_color_verts: Vec::with_capacity(4),
            bg_color_idxs: Vec::with_capacity(6),
            cell_bg_verts: Vec::with_capacity(cell_cap * 4),
            cell_bg_idxs: Vec::with_capacity(cell_cap * 6),
            text_verts: Vec::with_capacity(cell_cap * 4),
            text_idxs: Vec::with_capacity(cell_cap * 6),
            deco_verts: Vec::with_capacity(512 * 4),
            deco_idxs: Vec::with_capacity(512 * 6),
            cursor_verts: Vec::with_capacity(4),
            cursor_idxs: Vec::with_capacity(6),
        }
    }

    /// Clear all vertex arrays (reusing allocated capacity).
    pub fn clear(&mut self) {
        self.bg_color_verts.clear();
        self.bg_color_idxs.clear();
        self.cell_bg_verts.clear();
        self.cell_bg_idxs.clear();
        self.text_verts.clear();
        self.text_idxs.clear();
        self.deco_verts.clear();
        self.deco_idxs.clear();
        self.cursor_verts.clear();
        self.cursor_idxs.clear();
    }

    /// Build vertex arrays from a `RenderableContent` iterator.
    ///
    /// This is the main entry point: iterate all cells, build per-pass vertex
    /// arrays, then process the cursor.
    pub fn build<'a>(
        &mut self,
        mut content: RenderableContent<'a>,
        size_info: &SizeInfo,
        glyph_atlas: &mut WgpuGlyphAtlas,
    ) -> FrameBuildResult {
        self.clear();

        let screen_w = size_info.width();
        let screen_h = size_info.height();
        let cell_w = size_info.cell_width();
        let cell_h = size_info.cell_height();
        let pad_x = size_info.padding_x();
        let pad_y = size_info.padding_y();

        let full_redraw = false;

        // Iterate all renderable cells.
        for cell in content.by_ref() {
            let x = pad_x + cell.point.column.0 as f32 * cell_w;
            let y = pad_y + cell.point.line as f32 * cell_h;

            // --- Cell background (only if not the default background) ---
            if cell.bg_alpha > 0.0 {
                let color = rgb_to_f32_4(cell.bg, cell.bg_alpha);
                push_bg_quad(
                    &mut self.cell_bg_verts,
                    &mut self.cell_bg_idxs,
                    x,
                    y,
                    cell_w,
                    cell_h,
                    &color,
                );
            }

            // --- Decorations: underline / strikeout ---
            let flags = cell.flags;
            if flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT) {
                self.push_decoration(&cell, x, y, cell_w, cell_h);
            }

            // --- Text glyph ---
            if cell.character != ' ' || cell.extra.is_some() {
                let fg_color = rgb_to_f32_4(cell.fg, 1.0);
                let wide = flags.contains(Flags::WIDE_CHAR);
                let gw = if wide { cell_w * 2.0 } else { cell_w };
                self.push_text_glyph(
                    glyph_atlas,
                    0, // font_index: will be resolved by caller if needed
                    cell.character,
                    x,
                    y,
                    gw,
                    cell_h,
                    &fg_color,
                );

                // Zero-width characters rendered on top.
                if let Some(ref extra) = cell.extra {
                    if let Some(ref zerowidth) = extra.zerowidth {
                        for &zw_char in zerowidth {
                            self.push_text_glyph(
                                glyph_atlas,
                                0,
                                zw_char,
                                x,
                                y,
                                gw,
                                cell_h,
                                &fg_color,
                            );
                        }
                    }
                }
            }
        }

        // --- Extract background color before consuming content ---
        let bg_rgb = content.color(0); // NamedColor::Background = 0

        // --- Cursor ---
        let cursor = content.cursor();
        self.push_cursor(&cursor, size_info);

        // Build the full-screen background quad.
        let bg_color = [
            bg_rgb.r as f32 / 255.0,
            bg_rgb.g as f32 / 255.0,
            bg_rgb.b as f32 / 255.0,
            1.0,
        ];
        push_bg_quad_fullscreen(
            &mut self.bg_color_verts,
            &mut self.bg_color_idxs,
            screen_w,
            screen_h,
        );

        let screen_uniforms = ScreenUniforms::new(0.0, screen_w, screen_h);

        FrameBuildResult {
            screen_uniforms,
            bg_color,
            full_redraw,
        }
    }

    /// Build vertex arrays from pre-collected cells, a cursor, and background color.
    ///
    /// This variant avoids borrowing `RenderableContent` (which borrows `Display`),
    /// allowing the caller to extract cells first and release the Display borrow.
    pub fn build_from_cells(
        &mut self,
        cells: Vec<RenderableCell>,
        cursor: &RenderableCursor,
        bg_rgb: Rgb,
        size_info: &SizeInfo,
        glyph_atlas: &mut WgpuGlyphAtlas,
    ) -> FrameBuildResult {
        self.clear();

        let screen_w = size_info.width();
        let screen_h = size_info.height();
        let cell_w = size_info.cell_width();
        let cell_h = size_info.cell_height();
        let pad_x = size_info.padding_x();
        let pad_y = size_info.padding_y();

        let full_redraw = false;

        // Iterate all renderable cells.
        for cell in cells {
            let x = pad_x + cell.point.column.0 as f32 * cell_w;
            let y = pad_y + cell.point.line as f32 * cell_h;

            // --- Cell background (only if not the default background) ---
            if cell.bg_alpha > 0.0 {
                let color = rgb_to_f32_4(cell.bg, cell.bg_alpha);
                push_bg_quad(
                    &mut self.cell_bg_verts,
                    &mut self.cell_bg_idxs,
                    x,
                    y,
                    cell_w,
                    cell_h,
                    &color,
                );
            }

            // --- Decorations: underline / strikeout ---
            let flags = cell.flags;
            if flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT) {
                self.push_decoration(&cell, x, y, cell_w, cell_h);
            }

            // --- Text glyph ---
            if cell.character != ' ' || cell.extra.is_some() {
                let fg_color = rgb_to_f32_4(cell.fg, 1.0);
                let wide = flags.contains(Flags::WIDE_CHAR);
                let gw = if wide { cell_w * 2.0 } else { cell_w };
                self.push_text_glyph(
                    glyph_atlas,
                    0,
                    cell.character,
                    x,
                    y,
                    gw,
                    cell_h,
                    &fg_color,
                );

                if let Some(ref extra) = cell.extra {
                    if let Some(ref zerowidth) = extra.zerowidth {
                        for &zw_char in zerowidth {
                            self.push_text_glyph(
                                glyph_atlas,
                                0,
                                zw_char,
                                x,
                                y,
                                gw,
                                cell_h,
                                &fg_color,
                            );
                        }
                    }
                }
            }
        }

        // --- Cursor ---
        self.push_cursor(cursor, size_info);

        // Build the full-screen background quad.
        let bg_color = [
            bg_rgb.r as f32 / 255.0,
            bg_rgb.g as f32 / 255.0,
            bg_rgb.b as f32 / 255.0,
            1.0,
        ];
        push_bg_quad_fullscreen(
            &mut self.bg_color_verts,
            &mut self.bg_color_idxs,
            screen_w,
            screen_h,
        );

        let screen_uniforms = ScreenUniforms::new(0.0, screen_w, screen_h);

        FrameBuildResult {
            screen_uniforms,
            bg_color,
            full_redraw,
        }
    }

    // -----------------------------------------------------------------------
    // Private push helpers.
    // -----------------------------------------------------------------------

    /// Push decoration (underline / strikeout) quad(s) for a cell.
    fn push_decoration(
        &mut self,
        cell: &RenderableCell,
        x: f32,
        y: f32,
        cell_w: f32,
        cell_h: f32,
    ) {
        let color = rgb_to_f32_4(cell.underline, 1.0);
        let flags = cell.flags;

        // Underline bar: sits in the bottom ~15% of the cell.
        if flags.intersects(Flags::ALL_UNDERLINES) {
            let deco_h = (cell_h * 0.1).max(1.0);
            let deco_y = y + cell_h - deco_h;
            push_deco_quad(
                &mut self.deco_verts,
                &mut self.deco_idxs,
                x,
                deco_y,
                cell_w,
                deco_h,
                &color,
            );
        }

        // Strikeout: horizontal bar through the vertical center.
        if flags.contains(Flags::STRIKEOUT) {
            let deco_h = (cell_h * 0.1).max(1.0);
            let deco_y = y + cell_h * 0.45;
            push_deco_quad(
                &mut self.deco_verts,
                &mut self.deco_idxs,
                x,
                deco_y,
                cell_w,
                deco_h,
                &color,
            );
        }
    }

    /// Push a text glyph quad. Looks up the glyph in the atlas; if not cached,
    /// the caller should have populated it already (or we emit nothing).
    fn push_text_glyph(
        &mut self,
        atlas: &mut WgpuGlyphAtlas,
        font_index: usize,
        c: char,
        cell_x: f32,
        cell_y: f32,
        _cell_w: f32,
        _cell_h: f32,
        color: &[f32; 4],
    ) {
        if let Some(glyph) = atlas.get(font_index, c) {
            if glyph.width <= 0.0 || glyph.height <= 0.0 {
                return;
            }

            let gx = cell_x + glyph.x_offset;
            let gy = cell_y + glyph.y_offset;
            let gw = glyph.width;
            let gh = glyph.height;

            let idx_base = self.text_verts.len() as u32;

            // Four corners.
            self.text_verts.push(TextVertex {
                position: [gx, gy],
                tex_coords: [glyph.u, glyph.v],
                color: *color,
            });
            self.text_verts.push(TextVertex {
                position: [gx + gw, gy],
                tex_coords: [glyph.u2, glyph.v],
                color: *color,
            });
            self.text_verts.push(TextVertex {
                position: [gx + gw, gy + gh],
                tex_coords: [glyph.u2, glyph.v2],
                color: *color,
            });
            self.text_verts.push(TextVertex {
                position: [gx, gy + gh],
                tex_coords: [glyph.u, glyph.v2],
                color: *color,
            });

            push_quad_indices(&mut self.text_idxs, idx_base);
        }
    }

    /// Push cursor quad(s) based on cursor shape.
    fn push_cursor(&mut self, cursor: &RenderableCursor, size_info: &SizeInfo) {
        if cursor.shape() == CursorShape::Hidden {
            return;
        }

        let cell_w = size_info.cell_width();
        let cell_h = size_info.cell_height();
        let pad_x = size_info.padding_x();
        let pad_y = size_info.padding_y();
        let point = cursor.point();

        let x = pad_x + point.column.0 as f32 * cell_w;
        let y = pad_y + point.line as f32 * cell_h;
        let width_mult = cursor.width().get() as f32;
        let color = rgb_to_f32_4(cursor.color(), 1.0);

        match cursor.shape() {
            CursorShape::Block => {
                // Full cell block.
                push_cursor_quad(
                    &mut self.cursor_verts,
                    &mut self.cursor_idxs,
                    x,
                    y,
                    cell_w * width_mult,
                    cell_h,
                    &color,
                );
            },
            CursorShape::Beam => {
                // Vertical bar on the left edge.
                let beam_w = (cell_w * 0.12).max(1.5);
                push_cursor_quad(
                    &mut self.cursor_verts,
                    &mut self.cursor_idxs,
                    x,
                    y,
                    beam_w,
                    cell_h,
                    &color,
                );
            },
            CursorShape::Underline => {
                // Horizontal bar at the bottom.
                let line_h = (cell_h * 0.12).max(1.5);
                push_cursor_quad(
                    &mut self.cursor_verts,
                    &mut self.cursor_idxs,
                    x,
                    y + cell_h - line_h,
                    cell_w * width_mult,
                    line_h,
                    &color,
                );
            },
            CursorShape::HollowBlock => {
                // Draw a hollow rectangle: 4 thin quads forming the border.
                let border = (cell_w * 0.08).max(1.0);
                let qw = cell_w * width_mult;
                // Top
                push_cursor_quad(
                    &mut self.cursor_verts,
                    &mut self.cursor_idxs,
                    x,
                    y,
                    qw,
                    border,
                    &color,
                );
                // Bottom
                push_cursor_quad(
                    &mut self.cursor_verts,
                    &mut self.cursor_idxs,
                    x,
                    y + cell_h - border,
                    qw,
                    border,
                    &color,
                );
                // Left
                push_cursor_quad(
                    &mut self.cursor_verts,
                    &mut self.cursor_idxs,
                    x,
                    y,
                    border,
                    cell_h,
                    &color,
                );
                // Right
                push_cursor_quad(
                    &mut self.cursor_verts,
                    &mut self.cursor_idxs,
                    x + qw - border,
                    y,
                    border,
                    cell_h,
                    &color,
                );
            },
            CursorShape::Hidden => {},
        }
    }
}

impl Default for FrameBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Utility functions.
// ---------------------------------------------------------------------------

/// Convert `Rgb` + alpha to `[R, G, B, A]` in 0..1 range.
#[inline]
fn rgb_to_f32_4(rgb: Rgb, alpha: f32) -> [f32; 4] {
    [
        rgb.r as f32 / 255.0,
        rgb.g as f32 / 255.0,
        rgb.b as f32 / 255.0,
        alpha,
    ]
}

/// Push 6 indices for a quad (two triangles).
#[inline]
fn push_quad_indices(indices: &mut Vec<u32>, base: u32) {
    indices.push(base);
    indices.push(base + 1);
    indices.push(base + 2);
    indices.push(base);
    indices.push(base + 2);
    indices.push(base + 3);
}

/// Push a background quad (cell-level).
fn push_bg_quad(
    verts: &mut Vec<BgVertex>,
    indices: &mut Vec<u32>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: &[f32; 4],
) {
    let base = verts.len() as u32;
    verts.push(BgVertex { position: [x, y], color: *color });
    verts.push(BgVertex { position: [x + w, y], color: *color });
    verts.push(BgVertex { position: [x + w, y + h], color: *color });
    verts.push(BgVertex { position: [x, y + h], color: *color });
    push_quad_indices(indices, base);
}

/// Push a full-screen background quad covering [0, 0] to [screen_w, screen_h].
fn push_bg_quad_fullscreen(
    verts: &mut Vec<BgVertex>,
    indices: &mut Vec<u32>,
    screen_w: f32,
    screen_h: f32,
) {
    // Uniform white color; actual color comes from the uniform or draw clear.
    let color = [1.0, 1.0, 1.0, 1.0];
    let base = verts.len() as u32;
    verts.push(BgVertex { position: [0.0, 0.0], color });
    verts.push(BgVertex { position: [screen_w, 0.0], color });
    verts.push(BgVertex { position: [screen_w, screen_h], color });
    verts.push(BgVertex { position: [0.0, screen_h], color });
    push_quad_indices(indices, base);
}

/// Push a decoration quad.
fn push_deco_quad(
    verts: &mut Vec<DecoVertex>,
    indices: &mut Vec<u32>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: &[f32; 4],
) {
    let base = verts.len() as u32;
    verts.push(DecoVertex { position: [x, y], color: *color });
    verts.push(DecoVertex { position: [x + w, y], color: *color });
    verts.push(DecoVertex { position: [x + w, y + h], color: *color });
    verts.push(DecoVertex { position: [x, y + h], color: *color });
    push_quad_indices(indices, base);
}

/// Push a cursor quad.
fn push_cursor_quad(
    verts: &mut Vec<CursorVertex>,
    indices: &mut Vec<u32>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: &[f32; 4],
) {
    let base = verts.len() as u32;
    verts.push(CursorVertex { position: [x, y], color: *color });
    verts.push(CursorVertex { position: [x + w, y], color: *color });
    verts.push(CursorVertex { position: [x + w, y + h], color: *color });
    verts.push(CursorVertex { position: [x, y + h], color: *color });
    push_quad_indices(indices, base);
}
