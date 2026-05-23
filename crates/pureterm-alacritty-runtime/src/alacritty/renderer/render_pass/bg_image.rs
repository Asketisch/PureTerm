//! 背景图片渲染通道。
//!
//! 使用背景纹理（壁纸模式）渲染全屏四边形。
//! 固定 4 顶点全屏四边形，无需动态缓冲区。

use bytemuck::{Pod, Zeroable};
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{create_linear_sampler, create_simple_pipeline, create_texture_sampler_bgl, create_uniform_bgl, PassData, RenderPass};

/// 背景图片着色器使用的 uniform 结构体。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BgImageUniforms {
    pub corner_radius: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    pub opacity: f32,
    pub image_width: f32,
    pub image_height: f32,
    pub fit: f32,
    pub position: f32,
}

impl BgImageUniforms {
    pub fn new(
        corner_radius: f32,
        screen_width: f32,
        screen_height: f32,
    ) -> Self {
        Self {
            corner_radius,
            screen_width,
            screen_height,
            opacity: 1.0,
            image_width: 0.0,
            image_height: 0.0,
            fit: 2.0,
            position: 0.0,
        }
    }
}

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
    /// 当前 uniform 缓存，用于 set_params 局部更新。
    cached_uniforms: BgImageUniforms,
}

impl BgImagePass {
    /// 创建新的背景图片通道。
    pub fn new(
        device: &Device,
        queue: &Queue,
        config: &SurfaceConfiguration,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let uniform_bgl = create_uniform_bgl(
            device,
            "BgImage Uniform BGL",
            wgpu::ShaderStages::VERTEX_FRAGMENT,
        );
        let tex_sampler_bgl = create_texture_sampler_bgl(
            device,
            "BgImage Texture Sampler BGL",
            wgpu::ShaderStages::VERTEX_FRAGMENT,
        );

        let uniforms = BgImageUniforms::new(0.0, config.width as f32, config.height as f32);

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BgImage Uniform Buffer"),
            size: std::mem::size_of::<BgImageUniforms>() as u64,
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
            include_str!("../shaders/bg_image.wgsl"),
            BgImageVertex::desc(),
            &[&tex_sampler_bgl, &uniform_bgl],
            surface_format,
        );

        // 全屏四边形顶点（裁剪空间）：覆盖整个视口
        let vertices: [BgImageVertex; 4] = [
            BgImageVertex {
                position: [-1.0, -1.0],
                tex_coords: [0.0, 1.0],
            }, // 左下
            BgImageVertex {
                position: [1.0, -1.0],
                tex_coords: [1.0, 1.0],
            }, // 右下
            BgImageVertex {
                position: [-1.0, 1.0],
                tex_coords: [0.0, 0.0],
            }, // 左上
            BgImageVertex {
                position: [1.0, 1.0],
                tex_coords: [1.0, 0.0],
            }, // 右上
        ];

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BgImage Vertex Buffer"),
            size: 4 * std::mem::size_of::<BgImageVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BgImage Index Buffer"),
            size: 6 * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 写入静态全屏四边形数据
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(&FULLSCREEN_INDICES));
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
            cached_uniforms: uniforms,
        }
    }

    /// 设置背景图片纹理视图。传入 `None` 以隐藏。
    pub fn set_background(
        &mut self,
        device: &Device,
        texture_view: Option<&wgpu::TextureView>,
    ) {
        match texture_view {
            Some(view) => {
                let tex_sampler_bgl = create_texture_sampler_bgl(
                    device,
                    "BgImage Texture Sampler BGL",
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                );
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

    /// 更新图片参数（opacity, fit, position, 图片尺寸）。
    pub fn set_params(
        &mut self,
        queue: &Queue,
        opacity: f32,
        fit: f32,
        position: f32,
        image_w: f32,
        image_h: f32,
    ) {
        self.cached_uniforms.opacity = opacity;
        self.cached_uniforms.fit = fit;
        self.cached_uniforms.position = position;
        self.cached_uniforms.image_width = image_w;
        self.cached_uniforms.image_height = image_h;
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[self.cached_uniforms]));
    }

    /// 更新此帧的 uniforms（屏幕尺寸和圆角）。
    pub fn update(&mut self, queue: &Queue, data: &PassData) {
        self.cached_uniforms.corner_radius = data.corner_radius;
        self.cached_uniforms.screen_width = data.screen_width;
        self.cached_uniforms.screen_height = data.screen_height;
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[self.cached_uniforms]));
    }

    /// 背景图片当前是否可见。
    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

impl RenderPass for BgImagePass {
    fn label(&self) -> &'static str {
        "BgImage"
    }

    fn update_buffers(&mut self, _device: &Device, queue: &Queue, data: &PassData) {
        self.update(queue, data);
    }

    fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
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

    fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 固定全屏四边形，无需 resize 处理
    }
}
