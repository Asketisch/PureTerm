//! Multi-pass rendering module for PureTerm's GPU renderer.
//!
//! Replaces the single-pipeline approach with 8 separate render passes:
//! 1. Background Color - solid color quads for cell backgrounds
//! 2. Background Image - fullscreen quad with texture
//! 3. Cell Text - glyph quads sampling the grayscale atlas
//! 4. Kitty Image - inline image quads with RGBA textures
//! 5. Decorations - underline, strikethrough, overline quads
//! 6. Cursor - cursor quad with blink support
//! 7. Debug Overlay - text overlay without corner clipping
//! 8. Post Process - fullscreen vignette / effects

pub mod bg_color;
pub mod bg_image;
pub mod cell_text;
pub mod cursor;
pub mod debug_overlay;
pub mod decorations;
pub mod kitty_image;
pub mod post_process;

use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

/// Data shared between all passes for a single frame.
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

/// Screen-space uniforms shared by most passes.
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

/// Create a simple render pipeline with common settings.
///
/// Uses alpha blending, no depth/stencil, no culling, triangle list topology.
pub fn create_simple_pipeline(
    device: &Device,
    label: &str,
    shader_source: &str,
    vertex_layout: wgpu::VertexBufferLayout<'static>,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    surface_format: wgpu::TextureFormat,
) -> RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{} Layout", label)),
        bind_group_layouts,
        immediate_size: 0,
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
        multiview_mask: None,
        cache: None,
    })
}

/// Create a uniform bind group layout with a single uniform buffer at binding 0.
pub fn create_uniform_bgl(device: &Device, label: &str, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayout {
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

/// Create a texture + sampler bind group layout (bindings 0 and 1).
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

/// Create the standard nearest-neighbor sampler.
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

/// Create the standard linear sampler.
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

/// Pre-allocated max cells for buffer sizing.
/// 500×200 = 100K cells is far beyond any realistic terminal size.
const MAX_CELLS: u64 = 500 * 200;
/// Vertices per quad.
const VERTS_PER_QUAD: u64 = 4;
/// Indices per quad.
const INDICES_PER_QUAD: u64 = 6;

/// Calculate vertex buffer size for up to MAX_CELLS quads.
pub fn max_vertex_buffer_size<T: bytemuck::Pod>() -> u64 {
    MAX_CELLS * VERTS_PER_QUAD * std::mem::size_of::<T>() as u64
}

/// Calculate index buffer size for up to MAX_CELLS quads.
pub fn max_index_buffer_size() -> u64 {
    MAX_CELLS * INDICES_PER_QUAD * std::mem::size_of::<u32>() as u64
}

/// Calculate vertex buffer size for a fullscreen quad (4 vertices).
pub fn fullscreen_vertex_buffer_size<T: bytemuck::Pod>() -> u64 {
    4 * std::mem::size_of::<T>() as u64
}

/// Calculate index buffer size for a fullscreen quad (6 indices).
pub fn fullscreen_index_buffer_size() -> u64 {
    6 * std::mem::size_of::<u32>() as u64
}

/// A render pass trait. Each pass has its own pipeline and buffers.
pub trait RenderPass {
    fn label(&self) -> &'static str;
    fn update_buffers(&mut self, device: &Device, queue: &Queue, data: &PassData);
    fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>);
    fn resize(&mut self, device: &Device, config: &SurfaceConfiguration);
}
