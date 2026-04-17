//! 调试覆盖层渲染通道。
//!
//! 渲染调试信息的文本覆盖层。与 cell_text 相同的顶点格式，但无圆角裁剪。绑定灰度图集纹理。

use bytemuck::{Pod, Zeroable};
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_nearest_sampler, create_simple_pipeline, create_texture_sampler_bgl,
    max_index_buffer_size, max_vertex_buffer_size, PassData,
};

/// 调试文本四边形的顶点：位置 + 纹理坐标 + RGBA 颜色。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DebugVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
}

impl DebugVertex {
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

/// 调试覆盖层渲染通道。
pub struct DebugOverlayPass {
    pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    atlas_bind_group: Option<wgpu::BindGroup>,
    sampler: wgpu::Sampler,
    index_count: u32,
    visible: bool,
}

impl DebugOverlayPass {
    /// 创建新的调试覆盖层通道。
    pub fn new(device: &Device, _config: &SurfaceConfiguration, surface_format: wgpu::TextureFormat) -> Self {
        let tex_sampler_bgl = create_texture_sampler_bgl(device, "DebugOverlay Texture Sampler BGL", wgpu::ShaderStages::FRAGMENT);

        let sampler = create_nearest_sampler(device, "DebugOverlay Sampler");

        let pipeline = create_simple_pipeline(
            device,
            "DebugOverlay Pipeline",
            include_str!("../../shaders/debug_overlay.wgsl"),
            DebugVertex::desc(),
            &[Some(&tex_sampler_bgl)],
            surface_format,
        );

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("DebugOverlay Vertex Buffer"),
            size: max_vertex_buffer_size::<DebugVertex>(),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("DebugOverlay Index Buffer"),
            size: max_index_buffer_size(),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            atlas_bind_group: None,
            sampler,
            index_count: 0,
            visible: false,
        }
    }

    /// 设置调试文本渲染的图集纹理视图。
    pub fn set_atlas_texture(&mut self, device: &Device, atlas_view: &wgpu::TextureView) {
        let tex_sampler_bgl = create_texture_sampler_bgl(device, "DebugOverlay Texture Sampler BGL", wgpu::ShaderStages::FRAGMENT);
        self.atlas_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("DebugOverlay Atlas BG"),
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

    /// 更新此帧的顶点/索引数据。
    pub fn update(&mut self, queue: &Queue, vertices: &[DebugVertex], indices: &[u32], _data: &PassData) {
        self.index_count = indices.len() as u32;

        if !vertices.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(vertices));
        }
        if !indices.is_empty() {
            queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(indices));
        }
    }

    /// 渲染调试覆盖层文本。
    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.index_count == 0 || !self.visible {
            return;
        }
        let Some(ref atlas_bg) = self.atlas_bind_group else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, atlas_bg, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    /// 此通道的 resize 是空操作。
    pub fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 空操作：预分配的缓冲区处理任意大小
    }

    /// 设置调试覆盖层的可见性。
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// 获取当前可见性。
    pub fn is_visible(&self) -> bool {
        self.visible
    }
}
