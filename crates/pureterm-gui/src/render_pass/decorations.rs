//! 装饰渲染通道。
//!
//! 渲染下划线、删除线和上划线四边形。与 bg_color（位置 + 颜色）相同的顶点格式，带圆角裁剪。

use bytemuck::{Pod, Zeroable};
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_simple_pipeline, create_uniform_bgl, max_index_buffer_size, max_vertex_buffer_size,
    PassData, ScreenUniforms,
};

/// 装饰四边形的顶点：位置 + RGBA 颜色。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DecoVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

impl DecoVertex {
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

/// 装饰渲染通道。
pub struct DecorationsPass {
    pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    index_count: u32,
}

impl DecorationsPass {
    /// 创建新的装饰通道。
    pub fn new(device: &Device, queue: &Queue, config: &SurfaceConfiguration, surface_format: wgpu::TextureFormat) -> Self {
        let uniform_bgl = create_uniform_bgl(device, "Decorations Uniform BGL", wgpu::ShaderStages::FRAGMENT);

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Decorations Uniform Buffer"),
            size: std::mem::size_of::<ScreenUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Decorations Bind Group"),
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
            "Decorations Pipeline",
            include_str!("../../shaders/decorations.wgsl"),
            DecoVertex::desc(),
            &[Some(&uniform_bgl)],
            surface_format,
        );

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Decorations Vertex Buffer"),
            size: max_vertex_buffer_size::<DecoVertex>(),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Decorations Index Buffer"),
            size: max_index_buffer_size(),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniforms = ScreenUniforms::new(0.0, config.width as f32, config.height as f32);
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            bind_group,
            index_count: 0,
        }
    }

    /// 更新此帧的顶点/索引数据和 uniforms。
    pub fn update(&mut self, queue: &Queue, vertices: &[DecoVertex], indices: &[u32], data: &PassData) {
        self.index_count = indices.len() as u32;

        if !vertices.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(vertices));
        }
        if !indices.is_empty() {
            queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(indices));
        }

        let uniforms = ScreenUniforms::new(data.corner_radius, data.screen_width, data.screen_height);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    /// 渲染装饰四边形。
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

    /// 此通道的 resize 是空操作。
    pub fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 空操作：预分配的缓冲区处理任意大小
    }
}
