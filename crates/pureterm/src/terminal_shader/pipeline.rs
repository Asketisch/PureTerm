//! wgpu 渲染管线 + 每帧 GPU 资源。
//!
//! - [`TerminalPipeline`] 是 iced_wgpu `Pipeline` 的具体实现,持有所有共享
//!   渲染管线(bg_color / cell_text / bg_image / image_quad)、共享 bind group
//!   layout、共享 sampler 和持久化的字形/图像 atlas 纹理。
//! - [`GpuResources`] 是每个 `TerminalPrimitive` 实例自己拥有的 per-frame GPU
//!   缓冲区/BindGroup,用于支持分屏多 pane 场景下各自的 vertex buffer。
//! - [`create_pipeline`] 是创建 `wgpu::RenderPipeline` 的公共小工具。

use std::fmt;

use iced_wgpu::primitive::Pipeline;
use wgpu::util::DeviceExt;

use super::vertices::{BgImageVertex, BgVertex, ImageQuadVertex, TextVertex};

pub struct TerminalPipeline {
    // Shared render pipelines (immutable after creation)
    pub(super) bg_color_pipeline: wgpu::RenderPipeline,
    pub(super) text_pipeline: wgpu::RenderPipeline,

    // Shared bind group layouts
    pub(super) uniform_bgl: wgpu::BindGroupLayout,
    pub(super) text_atlas_bgl: wgpu::BindGroupLayout,
    pub(super) text_uniform_bgl: wgpu::BindGroupLayout,

    // Shared sampler
    pub(super) nearest_sampler: wgpu::Sampler,

    // Shared atlas textures (GlyphState is shared across all panes;
    // first pane to call prepare() uploads dirty atlas data).
    // Textures are persistent across frames; we only recreate them
    // when the atlas size changes (e.g., new glyphs push it to grow).
    // On steady-state frames we use `queue.write_texture` for partial updates.
    pub(super) grayscale_atlas_texture: Option<wgpu::Texture>,
    pub(super) color_atlas_texture: Option<wgpu::Texture>,
    pub(super) grayscale_atlas_size: (u32, u32),
    pub(super) color_atlas_size: (u32, u32),
    // Views derived from the textures above; the bind group is cached
    // and only re-created when the views change (i.e., on texture recreate).
    pub(super) grayscale_atlas_view: Option<wgpu::TextureView>,
    pub(super) color_atlas_view: Option<wgpu::TextureView>,
    pub(super) text_atlas_bind_group: Option<wgpu::BindGroup>,

    // GAP-54: background image pipeline + bind group layout.
    pub(super) bg_image_pipeline: wgpu::RenderPipeline,
    pub(super) bg_image_bgl: wgpu::BindGroupLayout,
    pub(super) bg_image_sampler: wgpu::Sampler,

    // T-core-graphics v3: image quad pipeline + atlas binding.
    //
    // The atlas is a single 2048×2048 RGBA8 texture (matching `ImageAtlas`
    // on the CPU side); every visible Sixel / chunked-graphics / inline
    // image lives somewhere inside it, packed by `ImageAtlas`'s row-stride allocator.
    // The CPU uploads dirty sub-rects each frame; this struct holds the
    // persistent GPU texture + view + bind group so we don't recreate them
    // when there's no new content.
    pub(super) image_quad_pipeline: wgpu::RenderPipeline,
    pub(super) image_atlas_bgl: wgpu::BindGroupLayout,
    pub(super) image_atlas_sampler: wgpu::Sampler,
    pub(super) image_atlas_texture: Option<wgpu::Texture>,
    pub(super) image_atlas_view: Option<wgpu::TextureView>,
    pub(super) image_atlas_size: (u32, u32),
    pub(super) image_atlas_bind_group: Option<wgpu::BindGroup>,
}

/// Per-frame GPU resources owned by each `TerminalPrimitive` instance.
/// Stored via `Mutex` because `Primitive::prepare()` receives `&self` and must be `Sync`.
pub(in crate::terminal_shader) struct GpuResources {
    pub(in crate::terminal_shader) bg_vertex_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) bg_index_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) bg_bind_group: Option<wgpu::BindGroup>,
    pub(in crate::terminal_shader) bg_index_count: u32,

    pub(in crate::terminal_shader) text_vertex_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) text_index_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) text_uniform_bind_group: Option<wgpu::BindGroup>,
    pub(in crate::terminal_shader) text_index_count: u32,

    // GAP-54: per-pane background image state.
    pub(in crate::terminal_shader) bg_image_vertex_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) bg_image_index_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) bg_image_index_count: u32,
    pub(in crate::terminal_shader) bg_image_bind_group: Option<wgpu::BindGroup>,
    pub(in crate::terminal_shader) bg_image_uniform_bind_group: Option<wgpu::BindGroup>,
    pub(in crate::terminal_shader) bg_image_texture: Option<wgpu::Texture>,
    pub(in crate::terminal_shader) bg_image_view: Option<wgpu::TextureView>,
    /// Hash of the loaded image bytes — used to skip GPU re-upload when
    /// the image hasn't changed across frames.
    pub(in crate::terminal_shader) bg_image_signature: u64,

    // T-core-graphics v3: per-pane image quad state.
    // Vertex/index buffers are rebuilt every frame from
    // `TerminalPrimitive::image_quad_draws`; the bind group references the
    // shared pipeline-level atlas texture (see `image_atlas_bind_group`).
    pub(in crate::terminal_shader) image_quad_vertex_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) image_quad_index_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) image_quad_uniform_bind_group: Option<wgpu::BindGroup>,
    pub(in crate::terminal_shader) image_quad_index_count: u32,
}

impl fmt::Debug for GpuResources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuResources")
            .field("bg_count", &self.bg_index_count)
            .field("text_count", &self.text_index_count)
            .finish()
    }
}

impl Default for GpuResources {
    fn default() -> Self {
        Self {
            bg_vertex_buffer: None,
            bg_index_buffer: None,
            bg_bind_group: None,
            bg_index_count: 0,
            text_vertex_buffer: None,
            text_index_buffer: None,
            text_uniform_bind_group: None,
            text_index_count: 0,
            bg_image_vertex_buffer: None,
            bg_image_index_buffer: None,
            bg_image_index_count: 0,
            bg_image_bind_group: None,
            bg_image_uniform_bind_group: None,
            bg_image_texture: None,
            bg_image_view: None,
            bg_image_signature: 0,
            image_quad_vertex_buffer: None,
            image_quad_index_buffer: None,
            image_quad_uniform_bind_group: None,
            image_quad_index_count: 0,
        }
    }
}

impl Pipeline for TerminalPipeline {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        // --- Shared uniform BGL ---
        let uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Terminal Uniform BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // --- BgColor pipeline ---
        let bg_shader_src = include_str!(
            "../../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/bg_color.wgsl"
        );
        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("BgColor Shader"),
            source: wgpu::ShaderSource::Wgsl(bg_shader_src.into()),
        });
        let bg_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("BgColor Layout"),
            bind_group_layouts: &[&uniform_bgl],
            push_constant_ranges: &[],
        });
        let bg_color_pipeline = create_pipeline(device, "BgColor", &bg_shader, &bg_layout, BgVertex::desc(), format);

        // --- CellText pipeline ---
        let text_atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Text Atlas BGL"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        let text_uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Text Uniform BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let text_shader_src = include_str!(
            "../../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/cell_text.wgsl"
        );
        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("CellText Shader"),
            source: wgpu::ShaderSource::Wgsl(text_shader_src.into()),
        });
        let text_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("CellText Layout"),
            bind_group_layouts: &[&text_atlas_bgl, &text_uniform_bgl],
            push_constant_ranges: &[],
        });
        let text_pipeline = create_pipeline(device, "CellText", &text_shader, &text_layout, TextVertex::desc(), format);

        // --- GAP-54: background image pipeline ---
        let bg_image_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("BgImage BGL"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bg_image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("BgImage Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let bg_image_shader_src = include_str!(
            "../../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/bg_image.wgsl"
        );
        let bg_image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("BgImage Shader"),
            source: wgpu::ShaderSource::Wgsl(bg_image_shader_src.into()),
        });
        let bg_image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("BgImage Layout"),
            bind_group_layouts: &[&uniform_bgl, &bg_image_bgl],
            push_constant_ranges: &[],
        });
        let bg_image_pipeline = create_pipeline(
            device,
            "BgImage",
            &bg_image_shader,
            &bg_image_layout,
            BgImageVertex::desc(),
            format,
        );

        // --- T-core-graphics v3: image quad pipeline ---
        let image_atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Image Atlas BGL"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let image_atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Image Atlas Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let image_quad_shader_src = include_str!(
            "../../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/image_quad.wgsl"
        );
        let image_quad_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ImageQuad Shader"),
            source: wgpu::ShaderSource::Wgsl(image_quad_shader_src.into()),
        });
        let image_quad_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ImageQuad Layout"),
            bind_group_layouts: &[&uniform_bgl, &image_atlas_bgl],
            push_constant_ranges: &[],
        });
        let image_quad_pipeline = create_pipeline(
            device,
            "ImageQuad",
            &image_quad_shader,
            &image_quad_layout,
            ImageQuadVertex::desc(),
            format,
        );

        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Text Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Self {
            bg_color_pipeline,
            uniform_bgl,
            text_pipeline,
            text_atlas_bgl,
            text_uniform_bgl,
            nearest_sampler,
            grayscale_atlas_texture: None,
            color_atlas_texture: None,
            grayscale_atlas_size: (0, 0),
            color_atlas_size: (0, 0),
            grayscale_atlas_view: None,
            color_atlas_view: None,
            text_atlas_bind_group: None,
            bg_image_pipeline,
            bg_image_bgl,
            bg_image_sampler,
            image_quad_pipeline,
            image_atlas_bgl,
            image_atlas_sampler,
            image_atlas_texture: None,
            image_atlas_view: None,
            image_atlas_size: (0, 0),
            image_atlas_bind_group: None,
        }
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    vertex_layout: wgpu::VertexBufferLayout<'static>,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[vertex_layout],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
        multiview: None,
        cache: None,
    })
}
