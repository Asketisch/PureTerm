//! Cursor render pass.
//!
//! Renders the terminal cursor quad with blink support. Toggles visibility every
//! 500ms via `update_blink()`.

use bytemuck::{Pod, Zeroable};
use std::time::{Duration, Instant};
use wgpu::{BindGroup, Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_simple_pipeline, create_uniform_bgl, max_index_buffer_size, max_vertex_buffer_size,
    PassData,
};

/// Vertex for cursor quads: position + RGBA color.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CursorVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

impl CursorVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = [
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x2,
        },
        wgpu::VertexAttribute {
            offset: std::mem::size_of::<[f32; 2]>() as u64,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x4,
        },
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// Cursor-specific uniforms with blink visibility.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CursorUniforms {
    pub corner_radius: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    pub blink_visible: f32,
}

/// Cursor render pass.
pub struct CursorPass {
    pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: BindGroup,
    index_count: u32,
    blink_visible: bool,
    blink_timer: Instant,
}

/// Blink interval in milliseconds.
const BLINK_INTERVAL_MS: u64 = 500;

impl CursorPass {
    /// Create a new cursor pass.
    pub fn new(device: &Device, queue: &Queue, config: &SurfaceConfiguration, surface_format: wgpu::TextureFormat) -> Self {
        let uniform_bgl = create_uniform_bgl(device, "Cursor Uniform BGL", wgpu::ShaderStages::FRAGMENT);

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cursor Uniform Buffer"),
            size: std::mem::size_of::<CursorUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Cursor Bind Group"),
            layout: &uniform_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform_buffer,
                    offset: 0,
                    size: None,
                }),
            }],
        });

        let pipeline = create_simple_pipeline(
            device,
            "Cursor Pipeline",
            include_str!("../../shaders/cursor.wgsl"),
            CursorVertex::desc(),
            &[Some(&uniform_bgl)],
            surface_format,
        );

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cursor Vertex Buffer"),
            size: max_vertex_buffer_size::<CursorVertex>(),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cursor Index Buffer"),
            size: max_index_buffer_size(),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Initialize uniforms
        let uniforms = CursorUniforms {
            corner_radius: 0.0,
            screen_width: config.width as f32,
            screen_height: config.height as f32,
            blink_visible: 1.0,
        };
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            bind_group,
            index_count: 0,
            blink_visible: true,
            blink_timer: Instant::now(),
        }
    }

    /// Update the blink timer. Call once per frame.
    /// Toggles `blink_visible` every 500ms.
    pub fn update_blink(&mut self) {
        if self.blink_timer.elapsed() >= Duration::from_millis(BLINK_INTERVAL_MS) {
            self.blink_visible = !self.blink_visible;
            self.blink_timer = Instant::now();
        }
    }

    /// Update vertex/index data and uniforms for this frame.
    pub fn update(&mut self, queue: &Queue, vertices: &[CursorVertex], indices: &[u32], data: &PassData) {
        self.index_count = indices.len() as u32;

        if !vertices.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(vertices));
        }
        if !indices.is_empty() {
            queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(indices));
        }

        let blink = if data.cursor_blink_visible {
            self.blink_visible as u32 as f32
        } else {
            1.0 // Always visible when blink is disabled
        };

        let uniforms = CursorUniforms {
            corner_radius: data.corner_radius,
            screen_width: data.screen_width,
            screen_height: data.screen_height,
            blink_visible: blink,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    /// Render the cursor quad.
    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    /// Resize is a no-op for this pass.
    pub fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // No-op: pre-allocated buffers handle any size
    }

    /// Get current blink visibility state.
    pub fn is_blink_visible(&self) -> bool {
        self.blink_visible
    }

    /// Force set blink visibility (e.g., reset on key press).
    pub fn reset_blink(&mut self) {
        self.blink_visible = true;
        self.blink_timer = Instant::now();
    }
}
