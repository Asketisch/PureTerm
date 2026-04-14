//! egui-wgpu backend for PureTerm settings overlay.

use std::sync::Arc;
use winit::event::WindowEvent;
use winit::window::Window;

/// Manages egui rendering within the wgpu pipeline.
pub struct EguiBackend {
    pub context: egui::Context,
    pub winit_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
}

impl EguiBackend {
    /// Create a new egui backend.
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        window: Arc<Window>,
    ) -> Self {
        let egui_ctx = egui::Context::default();
        let viewport_id = egui_ctx.viewport_id();

        let winit_state = egui_winit::State::new(
            egui_ctx.clone(),
            viewport_id,
            &*window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        let renderer = egui_wgpu::Renderer::new(device, surface_format, egui_wgpu::RendererOptions::default());

        Self {
            context: egui_ctx,
            winit_state,
            renderer,
        }
    }

    /// Process a winit window event through egui.
    /// Returns true if the event was consumed by egui.
    pub fn on_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        let response = self.winit_state.on_window_event(window, event);
        response.consumed
    }

    /// Run the egui update and get full output.
    pub fn run(
        &self,
        raw_input: egui::RawInput,
        update_fn: impl FnMut(&egui::Context),
    ) -> egui::FullOutput {
        self.context.run(raw_input, update_fn)
    }

    /// Render egui paint jobs onto an existing surface texture view.
    /// Uses LoadOp::Load to draw on top of existing content.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_view: &wgpu::TextureView,
        paint_jobs: Vec<egui::ClippedPrimitive>,
        textures_delta: egui::TexturesDelta,
        screen_descriptor: egui_wgpu::ScreenDescriptor,
    ) {
        // Upload egui textures
        for (id, image_delta) in &textures_delta.set {
            self.renderer.update_texture(device, queue, *id, image_delta);
        }

        // Create encoder and upload vertex/index buffers before render
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("egui Buffers Encoder"),
        });
        self.renderer.update_buffers(device, queue, &mut encoder, &paint_jobs, &screen_descriptor);

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut render_pass = render_pass.forget_lifetime();

            self.renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        queue.submit(std::iter::once(encoder.finish()));

        // Free textures that were removed
        for id in &textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}
