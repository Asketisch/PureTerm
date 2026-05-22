//! 背景色渲染通道。
//!
//! 为单元格背景渲染纯色四边形。使用位置 + 颜色顶点，通过 uniform 缓冲区进行圆角裁剪。
//! 缓冲区动态增长：初始 1024 顶点，需要时 2x 扩展。

use bytemuck::{Pod, Zeroable};
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_simple_pipeline, create_uniform_bgl, ensure_index_buffer, ensure_vertex_buffer,
    index_buffer_size, vertex_buffer_size, PassData, RenderPass,
    ScreenUniforms,
};

/// 背景色四边形的顶点：位置 + RGBA 颜色。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BgVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

impl BgVertex {
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

/// 背景色渲染通道的初始四边形容量。
const BG_INITIAL_QUADS: u64 = 1024;

/// 背景色渲染通道。
pub struct BgColorPass {
    pipeline: RenderPipeline,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertex_cap: u64,
    index_cap: u64,
    index_count: u32,
}

impl BgColorPass {
    /// 创建新的背景色通道。
    pub fn new(
        device: &Device,
        queue: &Queue,
        config: &SurfaceConfiguration,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let uniform_bgl = create_uniform_bgl(
            device,
            "BgColor Uniform BGL",
            wgpu::ShaderStages::FRAGMENT,
        );

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BgColor Uniform Buffer"),
            size: std::mem::size_of::<ScreenUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BgColor Bind Group"),
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
            "BgColor Pipeline",
            include_str!("../shaders/bg_color.wgsl"),
            BgVertex::desc(),
            &[&uniform_bgl],
            surface_format,
        );

        let initial_cap = BG_INITIAL_QUADS;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BgColor Vertex Buffer"),
            size: vertex_buffer_size::<BgVertex>(initial_cap),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BgColor Index Buffer"),
            size: index_buffer_size(initial_cap),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 使用当前屏幕大小初始化 uniforms
        let uniforms = ScreenUniforms::new(0.0, config.width as f32, config.height as f32);
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        Self {
            pipeline,
            vertex_buffer: Some(vertex_buffer),
            index_buffer: Some(index_buffer),
            uniform_buffer,
            bind_group,
            vertex_cap: initial_cap,
            index_cap: initial_cap,
            index_count: 0,
        }
    }

    /// 更新此帧的顶点/索引数据和 uniforms。
    pub fn update(
        &mut self,
        device: &Device,
        queue: &Queue,
        vertices: &[BgVertex],
        indices: &[u32],
        data: &PassData,
    ) {
        self.index_count = indices.len() as u32;

        // 计算需要的四边形数（每个四边形 4 个顶点）
        let needed_quads = ((vertices.len() as u64) + 3) / 4;
        ensure_vertex_buffer::<BgVertex>(
            device,
            "BgColor Vertex Buffer",
            &mut self.vertex_buffer,
            &mut self.vertex_cap,
            needed_quads,
        );
        ensure_index_buffer(
            device,
            "BgColor Index Buffer",
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

        // 更新 uniforms
        let uniforms =
            ScreenUniforms::new(data.corner_radius, data.screen_width, data.screen_height);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }
}

impl RenderPass for BgColorPass {
    fn label(&self) -> &'static str {
        "BgColor"
    }

    fn update_buffers(&mut self, _device: &Device, _queue: &Queue, _data: &PassData) {
        // 缓冲区更新在 update() 中处理
    }

    fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.index_count == 0 {
            return;
        }
        let Some(ref vb) = self.vertex_buffer else {
            return;
        };
        let Some(ref ib) = self.index_buffer else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 动态缓冲区在 update 时按需增长，resize 无需特殊处理
    }
}
