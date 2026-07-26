//! iced_wgpu `Primitive` 实现:`TerminalPrimitive` 是每帧由
//! `TerminalProgram::draw` 构建的一次性 GPU draw-call 集合,持有 bg vertices、
//! text vertices、atlas 脏数据与 image quad 共享句柄。
//!
//! 由于 Rust 不允许把同一 trait impl 拆到多个文件,这里保留
//! `impl Primitive for TerminalPrimitive` 作为外壳,把 `prepare()` 的实际逻辑
//! 委托给 [`super::primitive_prepare`] 与 [`super::primitive_quad_pass`] 中的
//! 自由函数,从而把单文件行数控制在警戒线以下。

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use iced::Rectangle;
use iced_wgpu::graphics::Viewport;
use iced_wgpu::primitive::Primitive;

use crate::background_image::{BackgroundMode, DecodedBackground};
use pureterm_alacritty_runtime::renderer::glyph_atlas::AtlasDirty;

use super::helpers::f32_to_bits;
use super::image_quad::{ImageQuadDraw, ImageQuadState};
use super::pipeline::{GpuResources, TerminalPipeline};
use super::primitive_prepare::{
    prepare_atlas_textures, prepare_bg_color_pass, prepare_bg_image_pass, prepare_cell_text_pass,
};
use super::primitive_quad_pass::{
    drain_decode_channel, prepare_image_atlas_texture, prepare_image_quad_buffers,
};
use super::vertices::{BgVertex, ScreenUniforms, TextVertex};

pub struct TerminalPrimitive {
    pub(in crate::terminal_shader) bg_vertices: Vec<BgVertex>,
    pub(in crate::terminal_shader) bg_indices: Vec<u32>,
    pub(in crate::terminal_shader) text_vertices: Vec<TextVertex>,
    pub(in crate::terminal_shader) text_indices: Vec<u32>,
    // Partial-upload dirty atlas (Fix 2): holds the dirty rect list + an
    // Arc-shared full atlas buffer. Render time loops the regions and emits
    // a `queue.write_texture` per region, avoiding the previous
    // `Vec::clone()` of the entire 4 MiB / 16 MiB atlas on every dirty frame.
    // `Arc` lets the main thread and render thread share the allocation.
    pub(in crate::terminal_shader) grayscale_atlas: Option<AtlasDirty>,
    pub(in crate::terminal_shader) color_atlas: Option<AtlasDirty>,
    /// Per-frame GPU buffers – each Primitive instance owns its own,
    /// avoiding the shared-Pipeline overwrite bug in split-screen.
    pub(in crate::terminal_shader) gpu: Mutex<GpuResources>,
    /// Shared scale factor: updated by `prepare()` with the actual viewport
    /// scale factor each frame; read by `draw()` to detect changes.
    pub(in crate::terminal_shader) viewport_scale_factor: Arc<AtomicU32>,
    /// GAP-54: cached background image, mode, opacity, and signature
    /// (FNV-1a of the loaded bytes). `None` when disabled or no file.
    pub(in crate::terminal_shader) background_image: Option<DecodedBackground>,
    pub(in crate::terminal_shader) background_mode: BackgroundMode,
    pub(in crate::terminal_shader) background_opacity: f32,
    pub(in crate::terminal_shader) background_signature: u64,

    // T-core-graphics v3: image quad state. Cloned from the shared `Arc`s
    // on `TerminalProgram` so the render thread can:
    //   1. Drain `decode_rx` into `image_quad_state` (push decoded RGBA
    //      pixels into the CPU-side `ImageAtlas`).
    //   2. Pull `image_quad_draws` (UI-thread filled) to build vertex/index
    //      buffers for the GPU atlas.
    //
    // The `Mutex` is short-lived (`try_recv()` / insert / clone-drain) and
    // never held across an `.await` — it just satisfies `Sync` for
    // `Primitive::prepare(&self, ...)` which only takes `&self`.
    pub(in crate::terminal_shader) image_quad_state: Arc<Mutex<ImageQuadState>>,
    pub(in crate::terminal_shader) image_quad_draws: Arc<Mutex<Vec<ImageQuadDraw>>>,
    pub(in crate::terminal_shader) decode_rx:
        Arc<Mutex<std::sync::mpsc::Receiver<crate::image_decode_worker::DecodedImage>>>,
    // Cell dimensions in logical px, cached at construction from
    // `glyph_state`. The image quad pass multiplies `col * cell_w` and
    // `row * cell_h` to get screen-space quads. Caching avoids taking the
    // glyph state lock inside `prepare()` (which runs on the render thread).
    pub(in crate::terminal_shader) cell_w: f32,
    pub(in crate::terminal_shader) cell_h: f32,
}

impl fmt::Debug for TerminalPrimitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalPrimitive")
            .field("bg_verts", &self.bg_vertices.len())
            .field("text_verts", &self.text_vertices.len())
            .field("gpu", &*self.gpu.lock().unwrap())
            .finish()
    }
}

impl TerminalPrimitive {
    /// GAP-54: install a new background image (decoded RGBA8). Pass
    /// `None` to clear. The signature is a caller-supplied hash of the
    /// source bytes — used to avoid re-uploading identical content
    /// across frames. Mode and opacity are applied to the texture at
    /// draw time; pass them every call.
    #[allow(dead_code)]
    pub fn set_background_image(
        &mut self,
        image: Option<DecodedBackground>,
        mode: BackgroundMode,
        opacity: f32,
        signature: u64,
    ) {
        self.background_image = image;
        self.background_mode = mode;
        self.background_opacity = opacity.clamp(0.0, 1.0);
        self.background_signature = signature;
    }
}

impl Primitive for TerminalPrimitive {
    type Pipeline = TerminalPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        // Publish the actual viewport scale factor so that `draw()` (next frame)
        // can reconfigure the glyph atlas if needed.
        let vp_scale = viewport.scale_factor();
        self.viewport_scale_factor.store(
            f32_to_bits(vp_scale),
            Ordering::Relaxed,
        );

        // Keep uniforms in logical coordinates — iced_wgpu sets the GPU viewport
        // to bounds × scale_factor, so the vertex shader's NDC conversion must use
        // the same logical coordinate system as the vertex positions.
        let uniforms = ScreenUniforms {
            corner_radius: 0.0,
            screen_width: bounds.width,
            screen_height: bounds.height,
            bg_opacity: 0.0,
            bg_mode: 0,
            bg_img_width: 0.0,
            bg_img_height: 0.0,
        };

        let mut gpu = self.gpu.lock().unwrap();

        // --- BgColor pass ---
        prepare_bg_color_pass(self, pipeline, device, bounds, &mut gpu, &uniforms);

        // --- GAP-54: background image pass ---
        prepare_bg_image_pass(self, pipeline, device, queue, bounds, &mut gpu);

        // --- Atlas textures (persistent) ---
        prepare_atlas_textures(self, pipeline, device, queue);

        // --- T-core-graphics v3: image quad pass ---
        // Step A: drain the decode channel.
        drain_decode_channel(self);
        // Step B: upload dirty atlas regions to the GPU texture.
        prepare_image_atlas_texture(self, pipeline, device, queue);
        // Step C: build per-frame vertex/index buffers from `image_quad_draws`.
        prepare_image_quad_buffers(self, pipeline, device, &mut gpu, &uniforms);

        // --- CellText pass ---
        prepare_cell_text_pass(self, pipeline, device, &mut gpu, &uniforms);

        drop(gpu); // release lock before any potential future use of self
    }

    fn draw(
        &self,
        pipeline: &Self::Pipeline,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        let gpu = self.gpu.lock().unwrap();

        // GAP-54: draw background image first (under everything). Reuses the
        // same ScreenUniforms (slot 0) and binds the per-pane image+sample (slot 1).
        if gpu.bg_image_index_count > 0 {
            if let (Some(vb), Some(ib), Some(img_bg), Some(uniform_bg)) = (
                &gpu.bg_image_vertex_buffer,
                &gpu.bg_image_index_buffer,
                &gpu.bg_image_bind_group,
                &gpu.bg_image_uniform_bind_group,
            ) {
                render_pass.set_pipeline(&pipeline.bg_image_pipeline);
                render_pass.set_bind_group(0, uniform_bg, &[]);
                render_pass.set_bind_group(1, img_bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.bg_image_index_count, 0, 0..1);
            }
        }

        // Draw bg_color pass
        if gpu.bg_index_count > 0 {
            if let (Some(vb), Some(ib), Some(bg)) = (&gpu.bg_vertex_buffer, &gpu.bg_index_buffer, &gpu.bg_bind_group) {
                render_pass.set_pipeline(&pipeline.bg_color_pipeline);
                render_pass.set_bind_group(0, bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.bg_index_count, 0, 0..1);
            }
        }

        // T-core-graphics v3: image quad pass. Rendered between bg_color
        // and cell_text so terminal text overlays images (z-order rule:
        // images as cell backgrounds, text on top — see ExitPlanMode plan
        // §0).
        if gpu.image_quad_index_count > 0 {
            if let (Some(vb), Some(ib), Some(atlas_bg), Some(uniform_bg)) = (
                &gpu.image_quad_vertex_buffer,
                &gpu.image_quad_index_buffer,
                &pipeline.image_atlas_bind_group,
                &gpu.image_quad_uniform_bind_group,
            ) {
                render_pass.set_pipeline(&pipeline.image_quad_pipeline);
                render_pass.set_bind_group(0, uniform_bg, &[]);
                render_pass.set_bind_group(1, atlas_bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.image_quad_index_count, 0, 0..1);
            }
        }

        // Draw cell_text pass
        if gpu.text_index_count > 0 {
            if let (Some(vb), Some(ib), Some(atlas_bg), Some(uniform_bg)) = (
                &gpu.text_vertex_buffer, &gpu.text_index_buffer,
                &pipeline.text_atlas_bind_group, &gpu.text_uniform_bind_group,
            ) {
                render_pass.set_pipeline(&pipeline.text_pipeline);
                render_pass.set_bind_group(0, atlas_bg, &[]);
                render_pass.set_bind_group(1, uniform_bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.text_index_count, 0, 0..1);
            }
        }

        true
    }
}
