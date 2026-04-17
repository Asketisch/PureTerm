//! 背景图片渲染通道。
//!
//! 使用背景纹理（壁纸模式）渲染全屏四边形。
//! 支持 BackgroundConfig 中的各种背景模式。

use bytemuck::{Pod, Zeroable};
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_linear_sampler, create_simple_pipeline, create_texture_sampler_bgl, create_uniform_bgl,
    fullscreen_index_buffer_size, fullscreen_vertex_buffer_size, PassData, ScreenUniforms,
};

/// 背景图片全屏四边形的顶点：位置 + 纹理坐标。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BgImageVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
}

impl BgImageVertex {
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

/// 全屏四边形索引。
const FULLSCREEN_INDICES: [u32; 6] = [0, 1, 2, 2, 1, 3];

/// 背景图片渲染通道。
pub struct BgImagePass {
    pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    bg_bind_group: Option<wgpu::BindGroup>,
    sampler: wgpu::Sampler,
    visible: bool,
}

impl BgImagePass {
    /// 创建新的背景图片通道。
    pub fn new(device: &Device, queue: &Queue, config: &SurfaceConfiguration, surface_format: wgpu::TextureFormat) -> Self {
        let uniform_bgl = create_uniform_bgl(device, "BgImage Uniform BGL", wgpu::ShaderStages::FRAGMENT);
        let tex_sampler_bgl = create_texture_sampler_bgl(device, "BgImage Texture Sampler BGL", wgpu::ShaderStages::FRAGMENT);

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BgImage Uniform Buffer"),
            size: std::mem::size_of::<ScreenUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BgImage Uniform BG"),
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

        let sampler = create_linear_sampler(device, "BgImage Sampler");

        let pipeline = create_simple_pipeline(
            device,
            "BgImage Pipeline",
            include_str!("../../shaders/bg_image.wgsl"),
            BgImageVertex::desc(),
            &[Some(&tex_sampler_bgl), Some(&uniform_bgl)],
            surface_format,
        );

        // 全屏四边形顶点（裁剪空间）：覆盖整个视口
        let vertices: [BgImageVertex; 4] = [
            BgImageVertex { position: [-1.0, -1.0], tex_coords: [0.0, 1.0] }, // 左下
            BgImageVertex { position: [1.0, -1.0], tex_coords: [1.0, 1.0] },  // 右下
            BgImageVertex { position: [-1.0, 1.0], tex_coords: [0.0, 0.0] },  // 左上
            BgImageVertex { position: [1.0, 1.0], tex_coords: [1.0, 0.0] },   // 右上
        ];

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BgImage Vertex Buffer"),
            size: fullscreen_vertex_buffer_size::<BgImageVertex>(),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BgImage Index Buffer"),
            size: fullscreen_index_buffer_size(),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 写入静态全屏四边形数据
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(&FULLSCREEN_INDICES));

        let uniforms = ScreenUniforms::new(0.0, config.width as f32, config.height as f32);
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            uniform_bind_group,
            bg_bind_group: None,
            sampler,
            visible: false,
        }
    }

    /// 设置背景图片纹理视图。传入 `None` 以隐藏。
    pub fn set_background(&mut self, device: &Device, texture_view: Option<&wgpu::TextureView>) {
        match texture_view {
            Some(view) => {
                let tex_sampler_bgl = create_texture_sampler_bgl(device, "BgImage Texture Sampler BGL", wgpu::ShaderStages::FRAGMENT);
                self.bg_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("BgImage Texture BG"),
                    layout: &tex_sampler_bgl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                }));
                self.visible = true;
            }
            None => {
                self.bg_bind_group = None;
                self.visible = false;
            }
        }
    }

    /// 更新全屏四边形的 UV 坐标（用于不同的背景模式）。
    pub fn set_uv_rect(&self, queue: &Queue, u0: f32, v0: f32, u1: f32, v1: f32) {
        let vertices: [BgImageVertex; 4] = [
            BgImageVertex { position: [-1.0, -1.0], tex_coords: [u0, v1] },
            BgImageVertex { position: [1.0, -1.0], tex_coords: [u1, v1] },
            BgImageVertex { position: [-1.0, 1.0], tex_coords: [u0, v0] },
            BgImageVertex { position: [1.0, 1.0], tex_coords: [u1, v0] },
        ];
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
    }

    /// 更新此帧的 uniforms。
    pub fn update(&mut self, queue: &Queue, data: &PassData) {
        let uniforms = ScreenUniforms::new(data.corner_radius, data.screen_width, data.screen_height);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    /// 渲染背景图片全屏四边形。
    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if !self.visible {
            return;
        }
        let Some(ref bg_bg) = self.bg_bind_group else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bg_bg, &[]);
        pass.set_bind_group(1, &self.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    /// 此通道的 resize 是空操作。
    pub fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 空操作：预分配的缓冲区处理任意大小
    }

    /// 背景图片当前是否可见。
    pub fn is_visible(&self) -> bool {
        self.visible
    }
}
