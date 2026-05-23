//! Independent render thread with message-passing architecture.
//!
//! The render thread owns the wgpu Device, Queue, and Surface, avoiding
//! borrow conflicts between the UI thread and GPU operations. It receives
//! vertex data through messages, updates GPU buffers, and executes the
//! multi-pass rendering pipeline.

use crossbeam_channel::{Receiver, Sender};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::render_pass::{
    bg_color::BgColorPass,
    bg_image::BgImagePass,
    cell_text::CellTextPass,
    cursor::CursorPass,
    debug_overlay::DebugOverlayPass,
    decorations::DecorationsPass,
    kitty_image::KittyImagePass,
    post_process::PostProcessPass,
    PassData, RenderPass,
};
use super::render_pass::{
    bg_color::BgVertex,
    cell_text::TextVertex,
    cursor::CursorVertex,
    decorations::DecoVertex,
    ScreenUniforms,
};

// ── Messages ─────────────────────────────────────────────────────────────────

/// Messages sent from the main thread to the render thread.
pub enum RenderMessage {
    /// Render a frame with the supplied vertex data.
    Frame {
        data: Box<FrameData>,
    },
    /// Window was resized; reconfigure the surface.
    Resize {
        width: u32,
        height: u32,
    },
    /// Set or replace the background image (raw RGBA bytes).
    SetBackgroundImage {
        data: Vec<u8>,
        width: u32,
        height: u32,
        opacity: f32,
        fit: f32,
        position: f32,
    },
    /// Clear the background image.
    ClearBackground,
    /// Shut down the render thread.
    Shutdown,
}

/// All vertex and uniform data needed for one frame.
pub struct FrameData {
    // Background colour pass (full-window clear colour quad + per-cell background).
    pub bg_color_verts: Vec<BgVertex>,
    pub bg_color_idxs: Vec<u32>,
    // Per-cell background quads.
    pub cell_bg_verts: Vec<BgVertex>,
    pub cell_bg_idxs: Vec<u32>,
    // Text glyph quads.
    pub text_verts: Vec<TextVertex>,
    pub text_idxs: Vec<u32>,
    // Decoration quads (underline, strikethrough, overline).
    pub deco_verts: Vec<DecoVertex>,
    pub deco_idxs: Vec<u32>,
    // Cursor quad.
    pub cursor_verts: Vec<CursorVertex>,
    pub cursor_idxs: Vec<u32>,
    // Shared screen-space uniforms.
    pub screen_uniforms: ScreenUniforms,
    // Terminal background colour used to clear the surface.
    pub bg_color: [f32; 4],
    // Whether this is a full redraw (forces all passes even if index count is zero).
    pub full_redraw: bool,
    // Whether the glyph atlas was updated this frame.
    pub atlas_dirty: bool,
    // Updated glyph atlas pixels (R8 grayscale), if changed.
    pub atlas_data: Option<Vec<u8>>,
    pub atlas_width: u32,
    pub atlas_height: u32,
    // Updated color atlas pixels (RGBA), if changed.
    pub color_atlas_data: Option<Vec<u8>>,
    pub color_atlas_width: u32,
    pub color_atlas_height: u32,
    pub color_atlas_dirty: bool,
}

// ── Handle ───────────────────────────────────────────────────────────────────

const MAILBOX_CAPACITY: usize = 64;

/// Cloneable handle for sending messages to the render thread.
#[derive(Clone)]
pub struct RenderThreadHandle {
    sender: Sender<RenderMessage>,
    stats: RenderStatsHandle,
}

impl RenderThreadHandle {
    /// Spawn the render thread, transferring ownership of wgpu resources.
    ///
    /// The caller is responsible for creating the Device, Queue, and Surface
    /// before calling this method. Once passed in, they are owned exclusively
    /// by the render thread.
    pub fn spawn(
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        passes: RenderPasses,
    ) -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(MAILBOX_CAPACITY);
        let stats = RenderStatsHandle::default();

        let stats_clone = stats.clone();
        std::thread::Builder::new()
            .name("pureterm-render".into())
            .spawn(move || {
                RenderThread::new(receiver, surface, device, queue, config, passes, stats_clone)
                    .run();
            })
            .expect("failed to spawn render thread");

        Self { sender, stats }
    }

    /// Send a raw message. Returns `false` if the channel is closed.
    pub fn send(&self, msg: RenderMessage) -> bool {
        self.sender.send(msg).is_ok()
    }

    /// Submit a frame for rendering.
    pub fn request_frame(&self, data: Box<FrameData>) {
        let _ = self.sender.send(RenderMessage::Frame { data });
    }

    /// Notify the render thread of a window resize.
    pub fn resize(&self, width: u32, height: u32) {
        let _ = self.sender.send(RenderMessage::Resize { width, height });
    }

    /// Shut down the render thread gracefully.
    pub fn shutdown(&self) {
        let _ = self.sender.send(RenderMessage::Shutdown);
    }

    /// Set or replace the background image.
    pub fn set_background_image(
        &self,
        data: Vec<u8>,
        width: u32,
        height: u32,
        opacity: f32,
        fit: f32,
        position: f32,
    ) {
        let _ = self.sender.send(RenderMessage::SetBackgroundImage {
            data,
            width,
            height,
            opacity,
            fit,
            position,
        });
    }

    /// Clear the background image.
    pub fn clear_background(&self) {
        let _ = self.sender.send(RenderMessage::ClearBackground);
    }

    /// Take a snapshot of the current render statistics.
    pub fn stats(&self) -> RenderStats {
        self.stats.snapshot()
    }
}

// ── Statistics ───────────────────────────────────────────────────────────────

/// Snapshot of render statistics for inspection / debug overlay.
#[derive(Default, Debug)]
pub struct RenderStats {
    pub fps: f32,
    pub frame_count: u64,
    pub draw_calls: u32,
    pub vertices: u32,
}

/// Atomic statistics handle shared between the render thread and callers.
#[derive(Default)]
pub struct RenderStatsHandle {
    fps: AtomicU32,
    frame_count: AtomicU64,
    draw_calls: AtomicU32,
    vertices: AtomicU32,
}

impl Clone for RenderStatsHandle {
    fn clone(&self) -> Self {
        Self {
            fps: AtomicU32::new(self.fps.load(Ordering::Relaxed)),
            frame_count: AtomicU64::new(self.frame_count.load(Ordering::Relaxed)),
            draw_calls: AtomicU32::new(self.draw_calls.load(Ordering::Relaxed)),
            vertices: AtomicU32::new(self.vertices.load(Ordering::Relaxed)),
        }
    }
}

impl RenderStatsHandle {
    pub fn snapshot(&self) -> RenderStats {
        RenderStats {
            fps: f32::from_bits(self.fps.load(Ordering::Relaxed)),
            frame_count: self.frame_count.load(Ordering::Relaxed),
            draw_calls: self.draw_calls.load(Ordering::Relaxed),
            vertices: self.vertices.load(Ordering::Relaxed),
        }
    }

    fn record_frame(&self, fps: f32, draw_calls: u32, vertices: u32) {
        self.fps.store(fps.to_bits(), Ordering::Relaxed);
        self.frame_count.fetch_add(1, Ordering::Relaxed);
        self.draw_calls.store(draw_calls, Ordering::Relaxed);
        self.vertices.store(vertices, Ordering::Relaxed);
    }
}

// ── Render Passes bundle ─────────────────────────────────────────────────────

/// All 8 render passes, grouped so they can be moved into the render thread.
pub struct RenderPasses {
    pub bg_color: BgColorPass,
    pub bg_image: BgImagePass,
    pub cell_text: CellTextPass,
    pub kitty_image: KittyImagePass,
    pub decorations: DecorationsPass,
    pub cursor: CursorPass,
    pub debug_overlay: DebugOverlayPass,
    pub post_process: PostProcessPass,
}

// ── Render Thread ────────────────────────────────────────────────────────────

/// Internal state of the render thread.
struct RenderThread {
    receiver: Receiver<RenderMessage>,

    // wgpu resources owned exclusively by this thread.
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    // The 8 render passes.
    passes: RenderPasses,

    // State.
    running: bool,

    // Cursor blink (500 ms toggle).
    cursor_blink_visible: bool,
    cursor_blink_timer: Instant,

    // FPS tracking.
    last_fps_time: Instant,
    frame_count: u64,
    current_fps: f32,

    // Stats shared with the handle.
    stats: RenderStatsHandle,
}

const BLINK_INTERVAL: Duration = Duration::from_millis(500);
const FPS_UPDATE_INTERVAL: Duration = Duration::from_secs(1);

impl RenderThread {
    fn new(
        receiver: Receiver<RenderMessage>,
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        passes: RenderPasses,
        stats: RenderStatsHandle,
    ) -> Self {
        Self {
            receiver,
            surface,
            device,
            queue,
            config,
            passes,
            running: true,
            cursor_blink_visible: true,
            cursor_blink_timer: Instant::now(),
            last_fps_time: Instant::now(),
            frame_count: 0,
            current_fps: 0.0,
            stats,
        }
    }

    // ── Main loop ────────────────────────────────────────────────────────────

    fn run(&mut self) {
        log::info!("Render thread started");

        while self.running {
            // Block until a message arrives.
            let msg = match self.receiver.recv() {
                Ok(msg) => msg,
                Err(_) => {
                    self.running = false;
                    break;
                }
            };

            let mut latest_frame = match msg {
                RenderMessage::Shutdown => {
                    self.running = false;
                    continue;
                }
                RenderMessage::Resize { width, height } => {
                    self.handle_resize(width, height);
                    None
                }
                RenderMessage::Frame { data } => Some(data),
                RenderMessage::SetBackgroundImage { data, width, height, opacity, fit, position } => {
                    self.handle_set_background_image(data, width, height, opacity, fit, position);
                    None
                }
                RenderMessage::ClearBackground => {
                    self.handle_clear_background();
                    None
                }
            };

            // Drain remaining messages, coalescing frames.
            while let Ok(msg) = self.receiver.try_recv() {
                match msg {
                    RenderMessage::Shutdown => {
                        self.running = false;
                        return;
                    }
                    RenderMessage::Resize { width, height } => {
                        self.handle_resize(width, height);
                    }
                    RenderMessage::Frame { mut data } => {
                        // Preserve atlas data from the frame being replaced.
                        if data.atlas_data.is_none() {
                            if let Some(ref prev) = latest_frame {
                                if prev.atlas_data.is_some() {
                                    data.atlas_data = prev.atlas_data.clone();
                                    data.atlas_width = prev.atlas_width;
                                    data.atlas_height = prev.atlas_height;
                                    data.atlas_dirty = true;
                                }
                            }
                        }
                        if data.color_atlas_data.is_none() {
                            if let Some(ref prev) = latest_frame {
                                if prev.color_atlas_data.is_some() {
                                    data.color_atlas_data = prev.color_atlas_data.clone();
                                    data.color_atlas_width = prev.color_atlas_width;
                                    data.color_atlas_height = prev.color_atlas_height;
                                    data.color_atlas_dirty = true;
                                }
                            }
                        }
                        latest_frame = Some(data);
                    }
                    RenderMessage::SetBackgroundImage { data, width, height, opacity, fit, position } => {
                        self.handle_set_background_image(data, width, height, opacity, fit, position);
                    }
                    RenderMessage::ClearBackground => {
                        self.handle_clear_background();
                    }
                }
            }

            self.update_blink();

            if let Some(frame_data) = latest_frame {
                self.draw_frame(&frame_data);
            }

            self.update_fps();
        }

        log::info!("Render thread shutting down");
    }

    // ── Message handlers ─────────────────────────────────────────────────────

    fn handle_resize(&mut self, width: u32, height: u32) {
        log::debug!("Render thread: resize {}x{}", width, height);
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);

        self.passes.bg_color.resize(&self.device, &self.config);
        self.passes.bg_image.resize(&self.device, &self.config);
        self.passes.cell_text.resize(&self.device, &self.config);
        self.passes.kitty_image.resize(&self.device, &self.config);
        self.passes.decorations.resize(&self.device, &self.config);
        self.passes.cursor.resize(&self.device, &self.config);
        self.passes.debug_overlay.resize(&self.device, &self.config);
        self.passes.post_process.resize(&self.device, &self.config);
    }

    fn handle_set_background_image(
        &mut self,
        data: Vec<u8>,
        width: u32,
        height: u32,
        opacity: f32,
        fit: f32,
        position: f32,
    ) {
        log::debug!("Render thread: set background image {}x{}", width, height);

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Background Image Texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.passes.bg_image.set_background(&self.device, Some(&view));
        self.passes
            .bg_image
            .set_params(&self.queue, opacity, fit, position, width as f32, height as f32);
    }

    fn handle_clear_background(&mut self) {
        log::debug!("Render thread: clear background");
        self.passes.bg_image.set_background(&self.device, None);
    }

    // ── Frame rendering ──────────────────────────────────────────────────────

    fn draw_frame(&mut self, frame: &FrameData) {
        // Upload grayscale glyph atlas if dirty.
        if let Some(ref atlas_bytes) = frame.atlas_data {
            self.upload_atlas(atlas_bytes, frame.atlas_width, frame.atlas_height, false);
        }

        // Upload color glyph atlas if dirty.
        if let Some(ref color_bytes) = frame.color_atlas_data {
            self.upload_atlas(
                color_bytes,
                frame.color_atlas_width,
                frame.color_atlas_height,
                true,
            );
        }

        // Build the shared pass data for this frame.
        let pass_data = PassData {
            screen_width: frame.screen_uniforms.screen_width,
            screen_height: frame.screen_uniforms.screen_height,
            corner_radius: frame.screen_uniforms.corner_radius,
            cursor_blink_visible: self.cursor_blink_visible,
            bg_image_texture: None,
            grayscale_atlas_texture: None,
            color_atlas_texture: None,
            scene_texture: None,
        };

        // Update pass buffers.
        self.passes.bg_color.update(
            &self.device,
            &self.queue,
            &frame.bg_color_verts,
            &frame.bg_color_idxs,
            &pass_data,
        );
        // Cell background shares the same vertex type as bg_color.
        if !frame.cell_bg_verts.is_empty() {
            self.passes.bg_color.update(
                &self.device,
                &self.queue,
                &frame.cell_bg_verts,
                &frame.cell_bg_idxs,
                &pass_data,
            );
        }

        self.passes.cell_text.update(
            &self.device,
            &self.queue,
            &frame.text_verts,
            &frame.text_idxs,
            &pass_data,
        );
        self.passes.decorations.update(
            &self.device,
            &self.queue,
            &frame.deco_verts,
            &frame.deco_idxs,
            &pass_data,
        );
        self.passes.cursor.update(
            &self.queue,
            &frame.cursor_verts,
            &frame.cursor_idxs,
            &pass_data,
        );
        self.passes.bg_image.update(&self.queue, &pass_data);
        self.passes.post_process.update(&self.queue, &pass_data);

        // Acquire the next surface texture.
        let surface_texture = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Timeout) => {
                log::warn!("Render thread: surface texture timeout");
                return;
            }
            Err(wgpu::SurfaceError::Outdated) => {
                log::warn!("Render thread: surface texture outdated, reconfiguring");
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(wgpu::SurfaceError::Lost) => {
                log::warn!("Render thread: surface texture lost, reconfiguring");
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                log::error!("Render thread: out of memory");
                return;
            }
            Err(e) => {
                log::warn!("Render thread: surface error {:?}", e);
                return;
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let encoder_descriptor = wgpu::CommandEncoderDescriptor {
            label: Some("PureTerm Frame Encoder"),
        };
        let mut encoder = self.device.create_command_encoder(&encoder_descriptor);

        // Clear the surface with the terminal background colour.
        let clear_color = wgpu::Color {
            r: frame.bg_color[0] as f64,
            g: frame.bg_color[1] as f64,
            b: frame.bg_color[2] as f64,
            a: frame.bg_color[3] as f64,
        };

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("PureTerm Main Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Execute passes in order.
            // Cursor is drawn before text so that inverted text (block cursor)
            // renders on top of the cursor rectangle.
            self.passes.bg_color.render(&mut render_pass);
            self.passes.bg_image.render(&mut render_pass);
            self.passes.cursor.render(&mut render_pass);
            self.passes.cell_text.render(&mut render_pass);
            self.passes.kitty_image.render(&mut render_pass);
            self.passes.decorations.render(&mut render_pass);
            self.passes.debug_overlay.render(&mut render_pass);
            self.passes.post_process.render(&mut render_pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        // Update frame counter.
        self.frame_count += 1;
    }

    fn upload_atlas(&mut self, data: &[u8], width: u32, height: u32, is_color: bool) {
        let format = if is_color {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::R8Unorm
        };
        let bytes_per_row = if is_color { width * 4 } else { width };
        let label = if is_color {
            "Color Glyph Atlas Upload"
        } else {
            "Glyph Atlas Upload"
        };

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Ensure color placeholder exists on first grayscale upload.
        if !is_color {
            self.passes.cell_text.ensure_color_placeholder(&self.device, &self.queue);
        }

        if is_color {
            self.passes.cell_text.update_color_atlas(&self.device, &view);
        } else {
            self.passes.cell_text.update_grayscale_atlas(&self.device, &view);
        }
    }

    // ── Cursor blink ─────────────────────────────────────────────────────────

    fn update_blink(&mut self) {
        if self.cursor_blink_timer.elapsed() >= BLINK_INTERVAL {
            self.cursor_blink_visible = !self.cursor_blink_visible;
            self.cursor_blink_timer = Instant::now();
        }
    }

    // ── FPS tracking ─────────────────────────────────────────────────────────

    fn update_fps(&mut self) {
        let elapsed = self.last_fps_time.elapsed();
        if elapsed >= FPS_UPDATE_INTERVAL {
            self.current_fps = self.frame_count as f32 / elapsed.as_secs_f32();
            self.frame_count = 0;
            self.last_fps_time = Instant::now();
            self.stats.record_frame(self.current_fps, 0, 0);
        }
    }
}
