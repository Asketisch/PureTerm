//! Kitty 图片渲染通道。
//!
//! 使用 RGBA 纹理采样渲染内联图片四边形（Kitty 协议）。
//! 每个图片四边形映射到一个 RGBA 纹理。

use bytemuck::{Pod, Zeroable};
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_linear_sampler, create_simple_pipeline, create_texture_sampler_bgl, create_uniform_bgl,
    max_index_buffer_size, max_vertex_buffer_size, PassData, ScreenUniforms,
};

/// 图片四边形的顶点：位置 + 纹理坐标 + 色调颜色。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ImageVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
}

impl ImageVertex {
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

/// Kitty 图片渲染通道。
pub struct KittyImagePass {
    pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    image_bind_group: Option<wgpu::BindGroup>,
    sampler: wgpu::Sampler,
    index_count: u32,
}

impl KittyImagePass {
    /// 创建新的 Kitty 图片通道。
    pub fn new(device: &Device, queue: &Queue, config: &SurfaceConfiguration, surface_format: wgpu::TextureFormat) -> Self {
        let uniform_bgl = create_uniform_bgl(device, "KittyImage Uniform BGL", wgpu::ShaderStages::FRAGMENT);
        let tex_sampler_bgl = create_texture_sampler_bgl(device, "KittyImage Texture Sampler BGL", wgpu::ShaderStages::FRAGMENT);

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("KittyImage Uniform Buffer"),
            size: std::mem::size_of::<ScreenUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("KittyImage Uniform BG"),
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

        let sampler = create_linear_sampler(device, "KittyImage Sampler");

        let pipeline = create_simple_pipeline(
            device,
            "KittyImage Pipeline",
            include_str!("../../shaders/kitty_image.wgsl"),
            ImageVertex::desc(),
            &[Some(&tex_sampler_bgl), Some(&uniform_bgl)],
            surface_format,
        );

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("KittyImage Vertex Buffer"),
            size: max_vertex_buffer_size::<ImageVertex>(),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("KittyImage Index Buffer"),
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
            uniform_bind_group,
            image_bind_group: None,
            sampler,
            index_count: 0,
        }
    }

    /// 设置要渲染的当前图片纹理。如果图片变化，在 `update()` 之前调用。
    pub fn set_image_texture(&mut self, device: &Device, texture_view: &wgpu::TextureView) {
        let tex_sampler_bgl = create_texture_sampler_bgl(device, "KittyImage Texture Sampler BGL", wgpu::ShaderStages::FRAGMENT);
        self.image_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("KittyImage Texture BG"),
            layout: &tex_sampler_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
    }

    /// 更新此帧的顶点/索引数据和 uniforms。
    pub fn update(&mut self, queue: &Queue, vertices: &[ImageVertex], indices: &[u32], data: &PassData) {
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

    /// 渲染图片四边形。
    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.index_count == 0 {
            return;
        }
        let Some(ref image_bg) = self.image_bind_group else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, image_bg, &[]);
        pass.set_bind_group(1, &self.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    /// 此通道的 resize 是空操作。
    pub fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 空操作：预分配的缓冲区处理任意大小
    }
}
