//! 光标渲染通道。
//!
//! 渲染带闪烁支持的终端光标四边形。通过 `update_blink()` 每 500ms 切换一次可见性。
//! 固定小缓冲区（16 顶点足够光标矩形）。

use bytemuck::{Pod, Zeroable};
use std::time::{Duration, Instant};
use wgpu::{BindGroup, Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_simple_pipeline, create_uniform_bgl, PassData, RenderPass,
};

/// 光标四边形的顶点：位置 + RGBA 颜色。
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

/// 带闪烁可见性的光标特定 uniforms。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CursorUniforms {
    pub corner_radius: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    pub blink_visible: f32,
}

/// 光标缓冲区的最大顶点数（16 顶点 = 4 个矩形，足够任何光标样式）。
const CURSOR_MAX_VERTS: u64 = 16;
/// 光标缓冲区的最大索引数。
const CURSOR_MAX_INDICES: u64 = 24;

/// 光标渲染通道。
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

/// 闪烁间隔（毫秒）。
const BLINK_INTERVAL_MS: u64 = 500;

impl CursorPass {
    /// 创建新的光标通道。
    pub fn new(
        device: &Device,
        queue: &Queue,
        config: &SurfaceConfiguration,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let uniform_bgl =
            create_uniform_bgl(device, "Cursor Uniform BGL", wgpu::ShaderStages::VERTEX_FRAGMENT);

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
            include_str!("../shaders/cursor.wgsl"),
            CursorVertex::desc(),
            &[&uniform_bgl],
            surface_format,
        );

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cursor Vertex Buffer"),
            size: CURSOR_MAX_VERTS * std::mem::size_of::<CursorVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cursor Index Buffer"),
            size: CURSOR_MAX_INDICES * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 初始化 uniforms
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

    /// 更新闪烁计时器。每帧调用一次。
    /// 每 500ms 切换一次 `blink_visible`。
    pub fn update_blink(&mut self) {
        if self.blink_timer.elapsed() >= Duration::from_millis(BLINK_INTERVAL_MS) {
            self.blink_visible = !self.blink_visible;
            self.blink_timer = Instant::now();
        }
    }

    /// 更新此帧的顶点/索引数据和 uniforms。
    pub fn update(
        &mut self,
        queue: &Queue,
        vertices: &[CursorVertex],
        indices: &[u32],
        data: &PassData,
    ) {
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
            1.0 // 禁用闪烁时始终可见
        };

        let uniforms = CursorUniforms {
            corner_radius: data.corner_radius,
            screen_width: data.screen_width,
            screen_height: data.screen_height,
            blink_visible: blink,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    /// 获取当前闪烁可见性状态。
    pub fn is_blink_visible(&self) -> bool {
        self.blink_visible
    }

    /// 强制设置闪烁可见性（例如，按键时重置）。
    pub fn reset_blink(&mut self) {
        self.blink_visible = true;
        self.blink_timer = Instant::now();
    }
}

impl RenderPass for CursorPass {
    fn label(&self) -> &'static str {
        "Cursor"
    }

    fn update_buffers(&mut self, _device: &Device, _queue: &Queue, _data: &PassData) {
        // 光标使用固定小缓冲区，在 update() 中直接写入
    }

    fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 固定小缓冲区，无需 resize
    }
}
