//! 单元格文本渲染通道。
//!
//! 通过从灰度字形图集采样来渲染字形四边形。使用位置 + 纹理坐标 + 颜色顶点。
//! 缓冲区动态增长：初始 10K 顶点，需要时 2x 扩展。

use bytemuck::{Pod, Zeroable};
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_nearest_sampler, create_simple_pipeline, create_texture_sampler_bgl, create_uniform_bgl,
    ensure_index_buffer, ensure_vertex_buffer, index_buffer_size, vertex_buffer_size, PassData,
    RenderPass, ScreenUniforms,
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

/// 单元格文本通道的初始四边形容量（10K 顶点 = 2500 个四边形）。
const TEXT_INITIAL_QUADS: u64 = 10_000 / 4;

/// 单元格文本渲染通道。
pub struct CellTextPass {
    pipeline: RenderPipeline,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    atlas_bind_group: Option<wgpu::BindGroup>,
    sampler: wgpu::Sampler,
    vertex_cap: u64,
    index_cap: u64,
    index_count: u32,
}

impl CellTextPass {
    /// 创建新的单元格文本通道。
    pub fn new(
        device: &Device,
        queue: &Queue,
        config: &SurfaceConfiguration,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let uniform_bgl = create_uniform_bgl(
            device,
            "CellText Uniform BGL",
            wgpu::ShaderStages::FRAGMENT,
        );
        let tex_sampler_bgl = create_texture_sampler_bgl(
            device,
            "CellText Texture Sampler BGL",
            wgpu::ShaderStages::FRAGMENT,
        );

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
            include_str!("../shaders/cell_text.wgsl"),
            TextVertex::desc(),
            &[&tex_sampler_bgl, &uniform_bgl],
            surface_format,
        );

        let initial_cap = TEXT_INITIAL_QUADS;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CellText Vertex Buffer"),
            size: vertex_buffer_size::<TextVertex>(initial_cap),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CellText Index Buffer"),
            size: index_buffer_size(initial_cap),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 初始化 uniforms
        let uniforms = ScreenUniforms::new(0.0, config.width as f32, config.height as f32);
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        Self {
            pipeline,
            vertex_buffer: Some(vertex_buffer),
            index_buffer: Some(index_buffer),
            uniform_buffer,
            bind_group: uniform_bg,
            atlas_bind_group: None,
            sampler,
            vertex_cap: initial_cap,
            index_cap: initial_cap,
            index_count: 0,
        }
    }

    /// 设置灰度图集纹理视图。在图集创建或更新时调用。
    pub fn set_atlas_texture(&mut self, device: &Device, atlas_view: &wgpu::TextureView) {
        let tex_sampler_bgl = create_texture_sampler_bgl(
            device,
            "CellText Texture Sampler BGL",
            wgpu::ShaderStages::FRAGMENT,
        );
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
    pub fn update(
        &mut self,
        device: &Device,
        queue: &Queue,
        vertices: &[TextVertex],
        indices: &[u32],
        data: &PassData,
    ) {
        self.index_count = indices.len() as u32;

        // 计算需要的四边形数
        let needed_quads = ((vertices.len() as u64) + 3) / 4;
        ensure_vertex_buffer::<TextVertex>(
            device,
            "CellText Vertex Buffer",
            &mut self.vertex_buffer,
            &mut self.vertex_cap,
            needed_quads,
        );
        ensure_index_buffer(
            device,
            "CellText Index Buffer",
            &mut self.index_buffer,
            &mut self.index_cap,
            needed_quads,
        );

        if !vertices.is_empty() {
            if let Some(ref vb) = self.vertex_buffer {
                queue.write_buffer(vb, 0, bytemuck::cast_slice(vertices));
            }
        }
        if !indices.is_empty() {
            if let Some(ref ib) = self.index_buffer {
                queue.write_buffer(ib, 0, bytemuck::cast_slice(indices));
            }
        }

        let uniforms =
            ScreenUniforms::new(data.corner_radius, data.screen_width, data.screen_height);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }
}

impl RenderPass for CellTextPass {
    fn label(&self) -> &'static str {
        "CellText"
    }

    fn update_buffers(&mut self, _device: &Device, _queue: &Queue, _data: &PassData) {
        // 缓冲区更新在 update() 中处理
    }

    fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        if let Some(ref atlas_bg) = self.atlas_bind_group {
            pass.set_bind_group(0, atlas_bg, &[]);
        }
        pass.set_bind_group(1, &self.bind_group, &[]);
        if let Some(ref vb) = self.vertex_buffer {
            pass.set_vertex_buffer(0, vb.slice(..));
        }
        if let Some(ref ib) = self.index_buffer {
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        }
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 动态缓冲区在 update 时按需增长，resize 无需特殊处理
    }
}
