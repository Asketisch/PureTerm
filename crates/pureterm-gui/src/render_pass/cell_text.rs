//! 单元格文本渲染通道。
//!
//! 通过从灰度字形图集采样来渲染字形四边形。使用位置 + 纹理坐标 + 颜色顶点。

use bytemuck::{Pod, Zeroable};
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_nearest_sampler, create_simple_pipeline, create_texture_sampler_bgl, create_uniform_bgl,
    max_index_buffer_size, max_vertex_buffer_size, PassData, ScreenUniforms,
};

/// 文本字形四边形的顶点：位置 + 纹理坐标 + RGBA 颜色。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TextVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
}

impl TextVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] = [
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x2,
        },
        wgpu::VertexAttribute {
            offset: std::mem::size_of::<[f32; 2]>() as u64,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x2,
        },
        wgpu::VertexAttribute {
            offset: std::mem::size_of::<[f32; 4]>() as u64,
            shader_location: 2,
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

/// 单元格文本渲染通道。
pub struct CellTextPass {
    pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    atlas_bind_group: Option<wgpu::BindGroup>,
    sampler: wgpu::Sampler,
    index_count: u32,
}

impl CellTextPass {
    /// 创建新的单元格文本通道。
    pub fn new(device: &Device, queue: &Queue, config: &SurfaceConfiguration, surface_format: wgpu::TextureFormat) -> Self {
        let uniform_bgl = create_uniform_bgl(device, "CellText Uniform BGL", wgpu::ShaderStages::FRAGMENT);
        let tex_sampler_bgl = create_texture_sampler_bgl(device, "CellText Texture Sampler BGL", wgpu::ShaderStages::FRAGMENT);

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CellText Uniform Buffer"),
            size: std::mem::size_of::<ScreenUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("CellText Uniform BG"),
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

        let sampler = create_nearest_sampler(device, "CellText Sampler");

        let pipeline = create_simple_pipeline(
            device,
            "CellText Pipeline",
            include_str!("../../shaders/cell_text.wgsl"),
            TextVertex::desc(),
            &[Some(&tex_sampler_bgl), Some(&uniform_bgl)],
            surface_format,
        );

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CellText Vertex Buffer"),
            size: max_vertex_buffer_size::<TextVertex>(),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CellText Index Buffer"),
            size: max_index_buffer_size(),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 初始化 uniforms
        let uniforms = ScreenUniforms::new(0.0, config.width as f32, config.height as f32);
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            bind_group: uniform_bg,
            atlas_bind_group: None,
            sampler,
            index_count: 0,
        }
    }

    /// 设置灰度图集纹理视图。在图集创建或更新时调用。
    pub fn set_atlas_texture(&mut self, device: &Device, atlas_view: &wgpu::TextureView) {
        let tex_sampler_bgl = create_texture_sampler_bgl(device, "CellText Texture Sampler BGL", wgpu::ShaderStages::FRAGMENT);
        self.atlas_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("CellText Atlas BG"),
            layout: &tex_sampler_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
    }

    /// 更新此帧的顶点/索引数据和 uniforms。
    pub fn update(&mut self, queue: &Queue, vertices: &[TextVertex], indices: &[u32], data: &PassData) {
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

    /// 渲染文本字形四边形。
    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        if let Some(ref atlas_bg) = self.atlas_bind_group {
            pass.set_bind_group(0, atlas_bg, &[]);
        }
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    /// 此通道的 resize 是空操作。
    pub fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 空操作：预分配的缓冲区处理任意大小
    }
}
