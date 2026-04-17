//! 后处理渲染通道。
//!
//! 对渲染的场景应用全屏效果（暗角、ShaderToy 自定义着色器）。采样场景纹理并写入表面。

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_linear_sampler, create_simple_pipeline, create_texture_sampler_bgl,
    create_uniform_bgl,
    PassData,
};

/// 后处理全屏四边形的顶点：位置 + 纹理坐标。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PostProcessVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
}

impl PostProcessVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = [
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
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// 后处理 uniforms：时间 + 屏幕大小 + 启用标志。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PostProcessUniforms {
    pub time: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    pub enabled: f32,
}

impl PostProcessUniforms {
    pub fn new(time: f32, screen_width: f32, screen_height: f32, enabled: bool) -> Self {
        Self {
            time,
            screen_width,
            screen_height,
            enabled: if enabled { 1.0 } else { 0.0 },
        }
    }
}

/// 全屏四边形顶点（带 UV 映射的 NDC 坐标）。
const FULLSCREEN_VERTICES: [PostProcessVertex; 4] = [
    PostProcessVertex { position: [-1.0, -1.0], tex_coords: [0.0, 1.0] },
    PostProcessVertex { position: [ 1.0, -1.0], tex_coords: [1.0, 1.0] },
    PostProcessVertex { position: [-1.0,  1.0], tex_coords: [0.0, 0.0] },
    PostProcessVertex { position: [ 1.0,  1.0], tex_coords: [1.0, 0.0] },
];

const FULLSCREEN_INDICES: [u32; 6] = [0, 1, 2, 2, 1, 3];

/// 后处理渲染通道。
pub struct PostProcessPass {
    pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    scene_bind_group: Option<wgpu::BindGroup>,
    uniform_bind_group: wgpu::BindGroup,
    scene_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    enabled: bool,
    start_time: std::time::Instant,
}

impl PostProcessPass {
    /// 创建新的后处理通道。
    pub fn new(device: &Device, _config: &SurfaceConfiguration, surface_format: wgpu::TextureFormat) -> Self {
        let scene_bgl = create_texture_sampler_bgl(
            device,
            "PostProcess Scene BGL",
            wgpu::ShaderStages::FRAGMENT,
        );

        let uniform_bgl = create_uniform_bgl(
            device,
            "PostProcess Uniform BGL",
            wgpu::ShaderStages::FRAGMENT,
        );

        let pipeline = create_simple_pipeline(
            device,
            "PostProcess Pipeline",
            include_str!("../../shaders/post_process.wgsl"),
            PostProcessVertex::desc(),
            &[Some(&scene_bgl), Some(&uniform_bgl)],
            surface_format,
        );

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("PostProcess Vertex Buffer"),
            contents: bytemuck::cast_slice(&FULLSCREEN_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("PostProcess Index Buffer"),
            contents: bytemuck::cast_slice(&FULLSCREEN_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("PostProcess Uniform Buffer"),
            size: std::mem::size_of::<PostProcessUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("PostProcess Uniform BG"),
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

        let sampler = create_linear_sampler(device, "PostProcess Sampler");

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            scene_bind_group: None,
            uniform_bind_group,
            scene_bgl,
            sampler,
            enabled: false,
            start_time: std::time::Instant::now(),
        }
    }

    /// 启用或禁用后处理效果。
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// 设置要采样的场景纹理。
    pub fn set_scene_texture(&mut self, device: &Device, view: wgpu::TextureView) {
        self.scene_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("PostProcess Scene BG"),
            layout: &self.scene_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
    }

    /// 更新此帧的 uniforms。
    pub fn update(&mut self, queue: &Queue, data: &PassData) {
        let elapsed = self.start_time.elapsed().as_secs_f32();
        let uniforms = PostProcessUniforms::new(elapsed, data.screen_width, data.screen_height, self.enabled);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    /// 渲染后处理全屏四边形。
    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let scene_bg = match &self.scene_bind_group {
            Some(bg) => bg,
            None => return,
        };

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, scene_bg, &[]);
        pass.set_bind_group(1, &self.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    /// 调整大小处理程序。
    pub fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        self.scene_bind_group = None;
    }
}
