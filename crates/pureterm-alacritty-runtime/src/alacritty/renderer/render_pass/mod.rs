//! PureTerm GPU 渲染器的多通道渲染模块。
//!
//! 用 8 个独立的渲染通道替代单管线方法：
//! 1. 背景色 - 单元格背景的纯色四边形
//! 2. 背景图片 - 带纹理的全屏四边形
//! 3. 单元格文本 - 从灰度字形图集采样的字形四边形
//! 4. Kitty 图片 - 带 RGBA 纹理的内联图片四边形
//! 5. 装饰 - 下划线、删除线、上划线四边形
//! 6. 光标 - 带闪烁支持的光标四边形
//! 7. 调试覆盖层 - 无角落裁剪的文本覆盖层
//! 8. 后处理 - 全屏暗角/效果
//!
//! 缓冲区策略：动态增长，初始小分配，需要时 2x 扩展。

pub mod bg_color;
pub mod bg_image;
pub mod cell_text;
pub mod cursor;
pub mod debug_overlay;
pub mod decorations;
pub mod kitty_image;
pub mod post_process;

use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

/// 单帧所有通道共享的数据。
#[derive(Default)]
pub struct PassData {
    pub screen_width: f32,
    pub screen_height: f32,
    pub corner_radius: f32,
    pub cursor_blink_visible: bool,
    pub bg_image_texture: Option<wgpu::TextureView>,
    pub grayscale_atlas_texture: Option<wgpu::TextureView>,
    pub color_atlas_texture: Option<wgpu::TextureView>,
    pub scene_texture: Option<wgpu::TextureView>,
}

/// 大多数通道共享的屏幕空间 uniforms。
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ScreenUniforms {
    pub corner_radius: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    pub _padding: f32,
}

impl ScreenUniforms {
    pub fn new(corner_radius: f32, screen_width: f32, screen_height: f32) -> Self {
        Self {
            corner_radius,
            screen_width,
            screen_height,
            _padding: 0.0,
        }
    }
}

/// 使用通用设置创建简单的渲染管线。
///
/// 使用 Alpha 混合，无深度/模板，无剔除，三角形列表拓扑。
pub fn create_simple_pipeline(
    device: &Device,
    label: &str,
    shader_source: &str,
    vertex_layout: wgpu::VertexBufferLayout<'static>,
    bind_group_layouts: &[&wgpu::BindGroupLayout],
    surface_format: wgpu::TextureFormat,
) -> RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{} Layout", label)),
        bind_group_layouts,
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[vertex_layout],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
        cache: None,
    })
}

/// 创建带有单个 uniform 缓冲区（绑定 0）的 uniform 绑定组布局。
pub fn create_uniform_bgl(
    device: &Device,
    label: &str,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// 创建纹理 + 采样器的绑定组布局（绑定 0 和 1）。
pub fn create_texture_sampler_bgl(
    device: &Device,
    label: &str,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// 创建标准最近邻采样器。
pub fn create_nearest_sampler(device: &Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    })
}

/// 创建标准线性采样器。
pub fn create_linear_sampler(device: &Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

/// 初始单元格数（缓冲区起始容量）。
const INITIAL_CELLS: u64 = 10_000;
/// 每个四边形的顶点数。
const VERTS_PER_QUAD: u64 = 4;
/// 每个四边形的索引数。
const INDICES_PER_QUAD: u64 = 6;

/// 计算指定四边形数量的顶点缓冲区大小。
pub fn vertex_buffer_size<T: bytemuck::Pod>(quad_count: u64) -> u64 {
    quad_count * VERTS_PER_QUAD * std::mem::size_of::<T>() as u64
}

/// 计算指定四边形数量的索引缓冲区大小。
pub fn index_buffer_size(quad_count: u64) -> u64 {
    quad_count * INDICES_PER_QUAD * std::mem::size_of::<u32>() as u64
}

/// 根据实际顶点数计算需要的四边形容量（向上取整到 2 的幂级别）。
/// 如果当前容量不足，返回 2x 当前容量（至少容纳需要的数量）。
pub fn grow_capacity(current: u64, needed_quads: u64) -> u64 {
    if needed_quads <= current {
        return current;
    }
    let mut new_cap = current.max(1);
    while new_cap < needed_quads {
        new_cap *= 2;
    }
    new_cap
}

/// 返回初始缓冲区分配的四边形数量。
pub fn initial_quad_capacity() -> u64 {
    INITIAL_CELLS
}

/// 创建一个动态增长的顶点缓冲区。如果当前缓冲区太小则重新分配。
pub fn ensure_vertex_buffer<T: bytemuck::Pod>(
    device: &Device,
    label: &str,
    current: &mut Option<wgpu::Buffer>,
    current_cap: &mut u64,
    needed_quads: u64,
) {
    let new_cap = grow_capacity(*current_cap, needed_quads);
    if new_cap != *current_cap || current.is_none() {
        *current_cap = new_cap;
        *current = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: vertex_buffer_size::<T>(new_cap),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
}

/// 创建一个动态增长的索引缓冲区。如果当前缓冲区太小则重新分配。
pub fn ensure_index_buffer(
    device: &Device,
    label: &str,
    current: &mut Option<wgpu::Buffer>,
    current_cap: &mut u64,
    needed_quads: u64,
) {
    let new_cap = grow_capacity(*current_cap, needed_quads);
    if new_cap != *current_cap || current.is_none() {
        *current_cap = new_cap;
        *current = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: index_buffer_size(new_cap),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
}

/// 渲染通道特征。每个通道都有自己的管线和缓冲区。
pub trait RenderPass {
    fn label(&self) -> &'static str;
    fn update_buffers(&mut self, device: &Device, queue: &Queue, data: &PassData);
    fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>);
    fn resize(&mut self, device: &Device, config: &SurfaceConfiguration);
}
