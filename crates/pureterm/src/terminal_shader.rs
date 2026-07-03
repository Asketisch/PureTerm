use bytemuck::{Pod, Zeroable};
use iced::widget::shader::{self, Action};
use iced::Rectangle;
use iced_wgpu::graphics::Viewport;
use iced_wgpu::primitive::{self, Pipeline, Primitive};
use wgpu::util::DeviceExt;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::fmt;
use std::sync::Arc;

use crate::background_image::{BackgroundMode, DecodedBackground};
use crate::terminal_context::TerminalContext;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as TermPoint};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use crossfont::{FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Style, Weight, Slant};
use pureterm_alacritty_runtime::renderer::glyph_atlas::{AtlasDirty, AtlasKind, CachedGlyph, WgpuGlyphAtlas};
use pureterm_alacritty_runtime::renderer::image_atlas::{AtlasDirty as ImageAtlasDirty, ImageAtlas, ImagePlacement};
use pureterm_alacritty_runtime::shaper::{ShapedRun, Shaper};

// 1x1 placeholder pixel data used when an atlas has no dirty content yet.
// Keeps the bind group valid even before the first glyph is rasterized.
const PLACEHOLDER_GRAY: &[u8] = &[0u8];
const PLACEHOLDER_COLOR: &[u8] = &[0u8, 0u8, 0u8, 0u8];

/// Shared scale factor written by `prepare()` (renderer thread) and read by
/// `draw()` (UI thread). Stored as `f32` bits in an `AtomicU32`.
fn f32_to_bits(f: f32) -> u32 { f.to_bits() }
fn bits_to_f32(u: u32) -> f32 { f32::from_bits(u) }

// --- Vertex types ---

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

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TextVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
    pub atlas_selector: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

// GAP-54: vertex for the background image pass (position + UV).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BgImageVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

impl BgImageVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = [
        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
        wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

impl TextVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 6] = [
        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
        wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
        wgpu::VertexAttribute { offset: 16, shader_location: 2, format: wgpu::VertexFormat::Float32x4 },
        wgpu::VertexAttribute { offset: 32, shader_location: 3, format: wgpu::VertexFormat::Float32 },
        wgpu::VertexAttribute { offset: 36, shader_location: 4, format: wgpu::VertexFormat::Float32 },
        wgpu::VertexAttribute { offset: 40, shader_location: 5, format: wgpu::VertexFormat::Float32 },
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ScreenUniforms {
    pub corner_radius: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    /// GAP-54: background image opacity in [0,1]; 0 = no image.
    pub bg_opacity: f32,
    /// GAP-54: 0 = Center, 1 = Tile, 2 = Stretch. u32 to keep 4-byte alignment.
    pub bg_mode: u32,
    /// GAP-54: image native dimensions in pixels (used for centering math).
    pub bg_img_width: f32,
    pub bg_img_height: f32,
}

// --- TerminalPipeline ---

pub struct TerminalPipeline {
    // Shared render pipelines (immutable after creation)
    bg_color_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,

    // Shared bind group layouts
    uniform_bgl: wgpu::BindGroupLayout,
    text_atlas_bgl: wgpu::BindGroupLayout,
    text_uniform_bgl: wgpu::BindGroupLayout,

    // Shared sampler
    nearest_sampler: wgpu::Sampler,

    // Shared atlas textures (GlyphState is shared across all panes;
    // first pane to call prepare() uploads dirty atlas data).
    // Textures are persistent across frames; we only recreate them
    // when the atlas size changes (e.g., new glyphs push it to grow).
    // On steady-state frames we use `queue.write_texture` for partial updates.
    grayscale_atlas_texture: Option<wgpu::Texture>,
    color_atlas_texture: Option<wgpu::Texture>,
    grayscale_atlas_size: (u32, u32),
    color_atlas_size: (u32, u32),
    // Views derived from the textures above; the bind group is cached
    // and only re-created when the views change (i.e., on texture recreate).
    grayscale_atlas_view: Option<wgpu::TextureView>,
    color_atlas_view: Option<wgpu::TextureView>,
    text_atlas_bind_group: Option<wgpu::BindGroup>,

    // GAP-54: background image pipeline + bind group layout.
    bg_image_pipeline: wgpu::RenderPipeline,
    bg_image_bgl: wgpu::BindGroupLayout,
    bg_image_sampler: wgpu::Sampler,
}

/// Per-frame GPU resources owned by each `TerminalPrimitive` instance.
/// Stored via `Mutex` because `Primitive::prepare()` receives `&self` and must be `Sync`.
struct GpuResources {
    bg_vertex_buffer: Option<wgpu::Buffer>,
    bg_index_buffer: Option<wgpu::Buffer>,
    bg_bind_group: Option<wgpu::BindGroup>,
    bg_index_count: u32,

    text_vertex_buffer: Option<wgpu::Buffer>,
    text_index_buffer: Option<wgpu::Buffer>,
    text_uniform_bind_group: Option<wgpu::BindGroup>,
    text_index_count: u32,

    // GAP-54: per-pane background image state.
    bg_image_vertex_buffer: Option<wgpu::Buffer>,
    bg_image_index_buffer: Option<wgpu::Buffer>,
    bg_image_index_count: u32,
    bg_image_bind_group: Option<wgpu::BindGroup>,
    bg_image_uniform_bind_group: Option<wgpu::BindGroup>,
    bg_image_texture: Option<wgpu::Texture>,
    bg_image_view: Option<wgpu::TextureView>,
    /// Hash of the loaded image bytes — used to skip GPU re-upload when
    /// the image hasn't changed across frames.
    bg_image_signature: u64,
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
            "../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/bg_color.wgsl"
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
            "../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/cell_text.wgsl"
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
            "../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/bg_image.wgsl"
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

// --- TerminalPrimitive ---

pub struct TerminalPrimitive {
    bg_vertices: Vec<BgVertex>,
    bg_indices: Vec<u32>,
    text_vertices: Vec<TextVertex>,
    text_indices: Vec<u32>,
    // Partial-upload dirty atlas (Fix 2): holds the dirty rect list + an
    // Arc-shared full atlas buffer. Render time loops the regions and emits
    // a `queue.write_texture` per region, avoiding the previous
    // `Vec::clone()` of the entire 4 MiB / 16 MiB atlas on every dirty frame.
    // `Arc` lets the main thread and render thread share the allocation.
    grayscale_atlas: Option<AtlasDirty>,
    color_atlas: Option<AtlasDirty>,
    /// Per-frame GPU buffers – each Primitive instance owns its own,
    /// avoiding the shared-Pipeline overwrite bug in split-screen.
    gpu: Mutex<GpuResources>,
    /// Shared scale factor: updated by `prepare()` with the actual viewport
    /// scale factor each frame; read by `draw()` to detect changes.
    viewport_scale_factor: Arc<AtomicU32>,
    /// GAP-54: cached background image, mode, opacity, and signature
    /// (FNV-1a of the loaded bytes). `None` when disabled or no file.
    background_image: Option<DecodedBackground>,
    background_mode: BackgroundMode,
    background_opacity: f32,
    background_signature: u64,
}

impl fmt::Debug for TerminalPrimitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalPrimitive")
            .field("bg_verts", &self.bg_vertices.len())
            .field("text_verts", &self.text_vertices.len())
            .field("gpu", &*self.gpu.lock().unwrap())
            .finish()
    }
}

impl TerminalPrimitive {
    /// GAP-54: install a new background image (decoded RGBA8). Pass
    /// `None` to clear. The signature is a caller-supplied hash of the
    /// source bytes — used to avoid re-uploading identical content
    /// across frames. Mode and opacity are applied to the texture at
    /// draw time; pass them every call.
    pub fn set_background_image(
        &mut self,
        image: Option<DecodedBackground>,
        mode: BackgroundMode,
        opacity: f32,
        signature: u64,
    ) {
        self.background_image = image;
        self.background_mode = mode;
        self.background_opacity = opacity.clamp(0.0, 1.0);
        self.background_signature = signature;
    }
}

impl Primitive for TerminalPrimitive {
    type Pipeline = TerminalPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        // Publish the actual viewport scale factor so that `draw()` (next frame)
        // can reconfigure the glyph atlas if needed.
        let vp_scale = viewport.scale_factor();
        self.viewport_scale_factor.store(
            f32_to_bits(vp_scale),
            Ordering::Relaxed,
        );

        // Keep uniforms in logical coordinates — iced_wgpu sets the GPU viewport
        // to bounds × scale_factor, so the vertex shader's NDC conversion must use
        // the same logical coordinate system as the vertex positions.
        let uniforms = ScreenUniforms {
            corner_radius: 0.0,
            screen_width: bounds.width,
            screen_height: bounds.height,
            bg_opacity: 0.0,
            bg_mode: 0,
            bg_img_width: 0.0,
            bg_img_height: 0.0,
        };

        let mut gpu = self.gpu.lock().unwrap();

        // --- BgColor pass ---
        if !self.bg_vertices.is_empty() {
            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("BG VB"), contents: bytemuck::cast_slice(&self.bg_vertices), usage: wgpu::BufferUsages::VERTEX,
            });
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("BG IB"), contents: bytemuck::cast_slice(&self.bg_indices), usage: wgpu::BufferUsages::INDEX,
            });
            let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("BG UB"), contents: bytemuck::cast_slice(&[uniforms]), usage: wgpu::BufferUsages::UNIFORM,
            });
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("BG Bind"), layout: &pipeline.uniform_bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
            });
            gpu.bg_vertex_buffer = Some(vb);
            gpu.bg_index_buffer = Some(ib);
            gpu.bg_bind_group = Some(bg);
            gpu.bg_index_count = self.bg_indices.len() as u32;
        } else {
            gpu.bg_index_count = 0;
        }

        // --- GAP-54: background image pass ---
        // Build vertices + indices + uniform + texture bind group. Skip
        // when no image is installed or opacity is 0.
        let bg_image = self.background_image.as_ref();
        if let (Some(img), true) = (bg_image, self.background_opacity > 0.0) {
            let mode = self.background_mode;
            let opacity = self.background_opacity;
            // Build the screen-space quad for the image. Modes:
            //   Center  → sub-rect of the viewport at image native size
            //   Tile    → repeat the viewport (uv wraps via Repeat sampler)
            //   Stretch → full-viewport quad with uv 0..1
            let (vp_w, vp_h) = (bounds.width, bounds.height);
            let (iw, ih) = (img.width as f32, img.height as f32);
            let (qx, qy, qw, qh) = match mode {
                BackgroundMode::Center => {
                    // Clamp to viewport.
                    let w = iw.min(vp_w);
                    let h = ih.min(vp_h);
                    let x = ((vp_w - w) * 0.5).max(0.0);
                    let y = ((vp_h - h) * 0.5).max(0.0);
                    (x, y, w, h)
                }
                BackgroundMode::Stretch | BackgroundMode::Tile => (0.0, 0.0, vp_w, vp_h),
            };
            let verts = [
                BgImageVertex { position: [qx,        qy       ], uv: [0.0, 0.0] },
                BgImageVertex { position: [qx + qw,   qy       ], uv: [1.0, 0.0] },
                BgImageVertex { position: [qx + qw,   qy + qh  ], uv: [1.0, 1.0] },
                BgImageVertex { position: [qx,        qy + qh  ], uv: [0.0, 1.0] },
            ];
            let idx = [0u32, 1, 2, 0, 2, 3];

            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("BgImage VB"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("BgImage IB"),
                contents: bytemuck::cast_slice(&idx),
                usage: wgpu::BufferUsages::INDEX,
            });

            // Uniform buffer (per-pane; uses screen dimensions + bg fields).
            let bg_uniforms = ScreenUniforms {
                corner_radius: 0.0,
                screen_width: vp_w,
                screen_height: vp_h,
                bg_opacity: opacity,
                bg_mode: match mode {
                    BackgroundMode::Center => 0,
                    BackgroundMode::Tile => 1,
                    BackgroundMode::Stretch => 2,
                },
                bg_img_width: iw,
                bg_img_height: ih,
            };
            let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("BgImage UB"),
                contents: bytemuck::cast_slice(&[bg_uniforms]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let ubg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("BgImage Uniform BG"),
                layout: &pipeline.uniform_bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
            });

            // Texture: upload (or skip) based on signature.
            if gpu.bg_image_signature != self.background_signature {
                let tex = device.create_texture_with_data(queue, &wgpu::TextureDescriptor {
                    label: Some("BgImage Tex"),
                    size: wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                }, wgpu::util::TextureDataOrder::LayerMajor, &img.data);
                let view = tex.create_view(&Default::default());
                let ibg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("BgImage Tex BG"),
                    layout: &pipeline.bg_image_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&pipeline.bg_image_sampler) },
                    ],
                });
                gpu.bg_image_texture = Some(tex);
                gpu.bg_image_view = Some(view);
                gpu.bg_image_bind_group = Some(ibg);
                gpu.bg_image_signature = self.background_signature;
            }

            gpu.bg_image_vertex_buffer = Some(vb);
            gpu.bg_image_index_buffer = Some(ib);
            gpu.bg_image_uniform_bind_group = Some(ubg);
            gpu.bg_image_index_count = idx.len() as u32;
        } else {
            gpu.bg_image_index_count = 0;
            // Drop per-frame buffers but keep the texture (signature still valid).
            gpu.bg_image_vertex_buffer = None;
            gpu.bg_image_index_buffer = None;
            gpu.bg_image_uniform_bind_group = None;
        }

        // --- Atlas textures (persistent) ---
        // Per atlas, decide whether to (a) recreate the texture (first frame
        // or size grew) or (b) just upload the new dirty data via
        // queue.write_texture. The bind group is only rebuilt when a texture
        // is actually recreated, eliminating per-frame GPU churn.
        //
        // CRITICAL: when take_dirty_data() returns None on a steady-state
        // frame (no new glyphs), the existing texture/view/bind_group must
        // be preserved. The 1x1 placeholder is only used on the very first
        // frame when the texture is None — folding it into a per-frame size
        // check would cause the texture to be destroyed and re-created as
        // 1x1 on every static frame, wiping the glyph cache and rendering
        // text invisible (Fix 3.5).
        let mut views_changed = false;

        if let Some(atlas) = &self.grayscale_atlas {
            let (w, h) = (atlas.width, atlas.height);
            let need_recreate = pipeline.grayscale_atlas_texture.is_none()
                || pipeline.grayscale_atlas_size != (w, h);
            if need_recreate {
                let tex = device.create_texture_with_data(queue, &wgpu::TextureDescriptor {
                    label: Some("Gray Atlas"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1, sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                }, wgpu::util::TextureDataOrder::LayerMajor, &atlas.data);
                pipeline.grayscale_atlas_texture = Some(tex);
                pipeline.grayscale_atlas_size = (w, h);
                pipeline.grayscale_atlas_view = Some(
                    pipeline.grayscale_atlas_texture.as_ref().unwrap().create_view(&Default::default())
                );
                views_changed = true;
            } else if let Some(tex) = pipeline.grayscale_atlas_texture.as_ref() {
                // Same size, partial upload (Fix 2): one `write_texture` per
                // dirty rect, byte-offset into the Arc-shared atlas buffer.
                // Wgpu requires bytes_per_row to be a multiple of
                // COPY_BYTES_PER_ROW_ALIGNMENT (256); the atlas allocator
                // emits power-of-two widths so this holds.
                for region in &atlas.regions {
                    let byte_offset = (region.y as usize * w as usize + region.x as usize);
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: region.x, y: region.y, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &atlas.data[byte_offset..],
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(w), // R8Unorm = 1 byte per pixel
                            rows_per_image: Some(region.h),
                        },
                        wgpu::Extent3d {
                            width: region.w,
                            height: region.h,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
        } else if pipeline.grayscale_atlas_texture.is_none() {
            // First frame, no glyphs rasterized yet: create a 1x1 placeholder
            // so the bind group is valid. Subsequent frames without new data
            // fall through to the "do nothing" branch below.
            let tex = device.create_texture_with_data(queue, &wgpu::TextureDescriptor {
                label: Some("Gray Atlas Placeholder"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }, wgpu::util::TextureDataOrder::LayerMajor, PLACEHOLDER_GRAY);
            pipeline.grayscale_atlas_texture = Some(tex);
            pipeline.grayscale_atlas_size = (1, 1);
            pipeline.grayscale_atlas_view = Some(
                pipeline.grayscale_atlas_texture.as_ref().unwrap().create_view(&Default::default())
            );
            views_changed = true;
        }
        // Else: texture exists and no new data → preserve texture/view.

        if let Some(atlas) = &self.color_atlas {
            let (w, h) = (atlas.width, atlas.height);
            let need_recreate = pipeline.color_atlas_texture.is_none()
                || pipeline.color_atlas_size != (w, h);
            if need_recreate {
                let tex = device.create_texture_with_data(queue, &wgpu::TextureDescriptor {
                    label: Some("Color Atlas"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1, sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                }, wgpu::util::TextureDataOrder::LayerMajor, &atlas.data);
                pipeline.color_atlas_texture = Some(tex);
                pipeline.color_atlas_size = (w, h);
                pipeline.color_atlas_view = Some(
                    pipeline.color_atlas_texture.as_ref().unwrap().create_view(&Default::default())
                );
                views_changed = true;
            } else if let Some(tex) = pipeline.color_atlas_texture.as_ref() {
                // Same size, partial upload (Fix 2): one `write_texture` per
                // dirty rect, byte-offset into the Arc-shared atlas buffer.
                // RGBA = 4 bytes per pixel; bytes_per_row must be a multiple
                // of 256 (atlas width is a power of two so this holds).
                for region in &atlas.regions {
                    let byte_offset =
                        (region.y as usize * w as usize + region.x as usize) * 4;
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: region.x, y: region.y, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &atlas.data[byte_offset..],
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(w * 4), // Rgba8UnormSrgb = 4 bytes per pixel
                            rows_per_image: Some(region.h),
                        },
                        wgpu::Extent3d {
                            width: region.w,
                            height: region.h,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
        } else if pipeline.color_atlas_texture.is_none() {
            let tex = device.create_texture_with_data(queue, &wgpu::TextureDescriptor {
                label: Some("Color Atlas Placeholder"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }, wgpu::util::TextureDataOrder::LayerMajor, PLACEHOLDER_COLOR);
            pipeline.color_atlas_texture = Some(tex);
            pipeline.color_atlas_size = (1, 1);
            pipeline.color_atlas_view = Some(
                pipeline.color_atlas_texture.as_ref().unwrap().create_view(&Default::default())
            );
            views_changed = true;
        }
        // Else: texture exists and no new data → preserve texture/view.

        // Recreate atlas bind group only when views changed (texture was
        // recreated) or the bind group hasn't been built yet.
        if views_changed || pipeline.text_atlas_bind_group.is_none() {
            let gray_view = pipeline.grayscale_atlas_view.as_ref().unwrap();
            let color_view = pipeline.color_atlas_view.as_ref().unwrap();
            pipeline.text_atlas_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Atlas BG"),
                layout: &pipeline.text_atlas_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(gray_view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&pipeline.nearest_sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(color_view) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&pipeline.nearest_sampler) },
                ],
            }));
        }

        // --- CellText pass ---
        if !self.text_vertices.is_empty() {
            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Text VB"), contents: bytemuck::cast_slice(&self.text_vertices), usage: wgpu::BufferUsages::VERTEX,
            });
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Text IB"), contents: bytemuck::cast_slice(&self.text_indices), usage: wgpu::BufferUsages::INDEX,
            });
            let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Text UB"), contents: bytemuck::cast_slice(&[uniforms]), usage: wgpu::BufferUsages::UNIFORM,
            });
            gpu.text_uniform_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Text Uniform BG"), layout: &pipeline.text_uniform_bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
            }));
            gpu.text_vertex_buffer = Some(vb);
            gpu.text_index_buffer = Some(ib);
            gpu.text_index_count = self.text_indices.len() as u32;
        } else {
            gpu.text_index_count = 0;
        }

        drop(gpu); // release lock before any potential future use of self
    }

    fn draw(
        &self,
        pipeline: &Self::Pipeline,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        let gpu = self.gpu.lock().unwrap();

        // GAP-54: draw background image first (under everything). Reuses the
        // same ScreenUniforms (slot 0) and binds the per-pane image+sample (slot 1).
        if gpu.bg_image_index_count > 0 {
            if let (Some(vb), Some(ib), Some(img_bg), Some(uniform_bg)) = (
                &gpu.bg_image_vertex_buffer,
                &gpu.bg_image_index_buffer,
                &gpu.bg_image_bind_group,
                &gpu.bg_image_uniform_bind_group,
            ) {
                render_pass.set_pipeline(&pipeline.bg_image_pipeline);
                render_pass.set_bind_group(0, uniform_bg, &[]);
                render_pass.set_bind_group(1, img_bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.bg_image_index_count, 0, 0..1);
            }
        }

        // Draw bg_color pass
        if gpu.bg_index_count > 0 {
            if let (Some(vb), Some(ib), Some(bg)) = (&gpu.bg_vertex_buffer, &gpu.bg_index_buffer, &gpu.bg_bind_group) {
                render_pass.set_pipeline(&pipeline.bg_color_pipeline);
                render_pass.set_bind_group(0, bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.bg_index_count, 0, 0..1);
            }
        }

        // Draw cell_text pass
        if gpu.text_index_count > 0 {
            if let (Some(vb), Some(ib), Some(atlas_bg), Some(uniform_bg)) = (
                &gpu.text_vertex_buffer, &gpu.text_index_buffer,
                &pipeline.text_atlas_bind_group, &gpu.text_uniform_bind_group,
            ) {
                render_pass.set_pipeline(&pipeline.text_pipeline);
                render_pass.set_bind_group(0, atlas_bg, &[]);
                render_pass.set_bind_group(1, uniform_bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.text_index_count, 0, 0..1);
            }
        }

        true
    }
}

// --- GlyphState: font rasterizer + glyph atlas ---

pub struct GlyphState {
    pub atlas: WgpuGlyphAtlas,
    pub rasterizer: Rasterizer,
    pub font_key: FontKey,
    pub font_size: Size,
    /// Actual rasterization size (logical size × scale_factor) for HiDPI support.
    pub scaled_font_size: Size,
    pub cell_width: f32,
    pub cell_height: f32,
    pub font_descent: f32,
    pub scale_factor: f32,
    /// Stored font config for reconfigure without caller providing them again.
    pub font_family: String,
    pub size_pt: f32,
    /// T-core-shaper v2: pluggable shaper (Monospace by default, swappable
    /// for `RustyBuzzShaper` once the host wires a real font blob through
    /// `AppConfig::shaper`).
    pub shaper: Arc<dyn Shaper>,
}

impl GlyphState {
    pub fn new_with_config(font_family: &str, size_pt: f32, scale_factor: f32) -> Self {
        let mut rasterizer = Rasterizer::new().expect("Failed to create rasterizer");
        let font_size = Size::new(size_pt);
        let scaled_font_size = font_size.scale(scale_factor);
        let font_desc = FontDesc::new(font_family, Style::Description { slant: Slant::Normal, weight: Weight::Normal });
        let font_key = rasterizer.load_font(&font_desc, scaled_font_size).unwrap_or_else(|_| {
            let fallback = FontDesc::new("monospace", Style::Description { slant: Slant::Normal, weight: Weight::Normal });
            rasterizer.load_font(&fallback, scaled_font_size).expect("Failed to load fallback font")
        });
        let metrics = rasterizer.metrics(font_key, scaled_font_size).expect("Failed to get metrics");

        // Convert physical metrics back to logical for grid calculations
        let cell_width = (metrics.average_advance / scale_factor as f64).floor().max(1.0) as f32;
        let cell_height = (metrics.line_height / scale_factor as f64).floor().max(1.0) as f32;
        let font_descent = metrics.descent / scale_factor as f32;

        Self {
            atlas: WgpuGlyphAtlas::new(),
            rasterizer,
            font_key,
            font_size,
            scaled_font_size,
            cell_width,
            cell_height,
            font_descent,
            scale_factor,
            font_family: font_family.to_string(),
            size_pt,
            // T-core-shaper: v1 monospace cluster shaper; v2 swaps in
            // `RustyBuzzShaper` when `[shaper] backend = "rustybuzz"` is
            // set in the config (see `app_config.rs` B.4).
            shaper: Arc::new(pureterm_alacritty_runtime::shaper::MonospaceShaper::new()),
        }
    }

    /// T-core-shaper v2: install a custom shaper (e.g. a
    /// `RustyBuzzShaper` constructed from a real font blob).
    pub fn set_shaper(&mut self, shaper: Arc<dyn Shaper>) {
        self.shaper = shaper;
    }

    /// T-core-shaper v2: shape a string using the active shaper, returning
    /// the per-glyph advances the caller can use to step the cursor.
    pub fn shape_text(&self, text: &str) -> ShapedRun {
        self.shaper.shape(text, self.cell_width)
    }

    /// Reconfigure font family, size, and scale factor together.
    pub fn reconfigure_scaled(&mut self, font_family: &str, size_pt: f32, scale_factor: f32) {
        self.scale_factor = scale_factor;
        self.font_family = font_family.to_string();
        self.size_pt = size_pt;
        let font_size = Size::new(size_pt);
        let scaled_font_size = font_size.scale(scale_factor);
        let font_desc = FontDesc::new(font_family, Style::Description { slant: Slant::Normal, weight: Weight::Normal });
        let font_key = self.rasterizer.load_font(&font_desc, scaled_font_size).unwrap_or_else(|_| {
            let fallback = FontDesc::new("monospace", Style::Description { slant: Slant::Normal, weight: Weight::Normal });
            self.rasterizer.load_font(&fallback, scaled_font_size).expect("Failed to load fallback font")
        });
        let metrics = self.rasterizer.metrics(font_key, scaled_font_size).expect("Failed to get metrics");

        // Convert physical metrics back to logical for grid calculations
        self.font_key = font_key;
        self.font_size = font_size;
        self.scaled_font_size = scaled_font_size;
        self.cell_width = (metrics.average_advance / scale_factor as f64).floor().max(1.0) as f32;
        self.cell_height = (metrics.line_height / scale_factor as f64).floor().max(1.0) as f32;
        self.font_descent = metrics.descent / scale_factor as f32;
        self.atlas = WgpuGlyphAtlas::new();
    }

    /// Update only the scale factor, keeping the current font family and size.
    /// Returns true if the scale factor actually changed.
    pub fn update_scale_factor(&mut self, scale_factor: f32) -> bool {
        if (self.scale_factor - scale_factor).abs() < 0.01 {
            return false;
        }
        self.reconfigure_scaled(&self.font_family.clone(), self.size_pt, scale_factor);
        true
    }

    pub fn get_or_rasterize(&mut self, c: char) -> Option<CachedGlyph> {
        if let Some(cached) = self.atlas.get(0, c) {
            // Atlas stores physical-pixel glyphs; scale offsets to logical
            // for vertex positioning. Cache hit must apply the same
            // normalization as the new-rasterize path below — otherwise
            // on Retina (scale_factor=2.0) the consumer gets 2× the
            // expected logical width and the glyph overflows its cell.
            return Some(CachedGlyph {
                u: cached.u,
                v: cached.v,
                u2: cached.u2,
                v2: cached.v2,
                x_offset: cached.x_offset / self.scale_factor,
                y_offset: cached.y_offset / self.scale_factor,
                width: cached.width / self.scale_factor,
                height: cached.height / self.scale_factor,
                x_advance: cached.x_advance / self.scale_factor,
                y_advance: cached.y_advance / self.scale_factor,
                atlas_kind: cached.atlas_kind,
            });
        }

        let glyph_key = GlyphKey { character: c, font_key: self.font_key, size: self.scaled_font_size };
        match self.rasterizer.get_glyph(glyph_key) {
            Ok(glyph) => {
                let cached = self.atlas.insert(0, c, &glyph);
                // Atlas stores physical-pixel glyphs; scale offsets to logical for vertex positioning
                Some(CachedGlyph {
                    u: cached.u,
                    v: cached.v,
                    u2: cached.u2,
                    v2: cached.v2,
                    x_offset: cached.x_offset / self.scale_factor,
                    y_offset: cached.y_offset / self.scale_factor,
                    width: cached.width / self.scale_factor,
                    height: cached.height / self.scale_factor,
                    x_advance: cached.x_advance / self.scale_factor,
                    y_advance: cached.y_advance / self.scale_factor,
                    atlas_kind: cached.atlas_kind,
                })
            }
            Err(_) => None,
        }
    }

    pub fn mark_atlas_dirty(&mut self) {
        self.atlas.mark_dirty();
    }
}

/// T-core-graphics v2: image quad pass state.
///
/// The image atlas is the CPU-side RGBA bitmap that backs Sixel / Kitty
/// / iTerm2 inline images. The render thread uploads dirty regions
/// to the GPU once per frame (see `take_dirty_data()`).
///
/// v2 limitation: the GPU quad-rendering pipeline (vertex + fragment
/// shader pair that samples the image atlas with bilinear filtering)
/// is wired in v3. For now `ImageQuadState` only owns the CPU side;
/// callers can read placements but not yet draw them.
pub struct ImageQuadState {
    pub atlas: ImageAtlas,
}

impl ImageQuadState {
    pub fn new() -> Self {
        Self {
            atlas: ImageAtlas::new(),
        }
    }

    /// Insert a decoded RGBA image into the atlas. Returns the
    /// `ImagePlacement` the GPU quad pass will sample from.
    pub fn insert(&mut self, id: u64, rgba: std::sync::Arc<Vec<u8>>, w: u32, h: u32) -> Option<ImagePlacement> {
        self.atlas.insert(id, rgba, w, h)
    }

    /// Remove an image (Kitty `d=d`, scrollback prune, etc.).
    pub fn remove(&mut self, id: u64) -> bool {
        self.atlas.remove(id)
    }

    /// Look up the GPU placement for an already-cached image.
    pub fn get(&mut self, id: u64) -> Option<ImagePlacement> {
        self.atlas.get(id)
    }

    /// Returns the dirty upload payload if any. The render thread is
    /// responsible for `queue.write_texture` to the image atlas.
    pub fn take_dirty_data(&mut self) -> Option<ImageAtlasDirty> {
        self.atlas.take_dirty_data()
    }

    pub fn is_dirty(&self) -> bool {
        self.atlas.is_dirty()
    }

    pub fn len(&self) -> usize {
        self.atlas.len()
    }
}

impl Default for ImageQuadState {
    fn default() -> Self {
        Self::new()
    }
}

// --- TerminalProgram ---

pub struct TerminalProgram {
    pub ctx: Arc<TerminalContext>,
    pub glyph_state: Arc<RefCell<GlyphState>>,
    pub background_color: [f32; 4],
    pub foreground_color: [f32; 4],
    pub opacity: f32,
    /// Shared atomic for the *actual* viewport scale factor, written by
    /// `prepare()` each frame. `draw()` reads it and reconfigures the
    /// glyph atlas when the value changes.
    pub viewport_scale_factor: Arc<AtomicU32>,
    /// GAP-4.1: Shared vi search pattern for match highlights.
    /// The app sets this when the user types in the search bar;
    /// the shader reads it during `draw()` to highlight matches.
    pub vi_search_pattern: Arc<std::sync::Mutex<Option<String>>>,
    /// GAP-3.8/4.3: Cached built `RegexSearch` keyed on the pattern string.
    /// Without this, `draw()` rebuilt 4 lazy DFAs every frame (terminal_shader
    /// ran `RegexSearch::new` per frame). The app owns the `Arc` and clones it
    /// into each per-frame `TerminalProgram`; the cache therefore persists
    /// across frames. Rebuilt only when the pattern string changes.
    pub cached_search:
        Arc<std::sync::Mutex<Option<(String, alacritty_terminal::term::search::RegexSearch)>>>,
    /// GAP-10: Trigger highlight regions for the active pane (published each frame
    /// by the app into this shared `Arc`, mirroring `vi_search_pattern`).
    pub trigger_highlights:
        Arc<std::sync::Mutex<Vec<crate::triggers::HighlightRegion>>>,
    /// GAP-10: collapsed fold regions for the active pane.
    pub trigger_folds: Arc<std::sync::Mutex<Vec<crate::triggers::FoldRegion>>>,

    // GAP-54: shared background image (loaded once, cloned into each per-frame
    // primitive). The signature lets the primitive skip re-uploading the GPU
    // texture when nothing changed.
    pub background_image: Arc<std::sync::Mutex<Option<DecodedBackground>>>,
    pub background_mode: BackgroundMode,
    pub background_opacity: f32,
    pub background_signature: u64,
}

#[derive(Default)]
pub struct TerminalState;

impl<M> shader::Program<M> for TerminalProgram {
    type State = TerminalState;
    type Primitive = TerminalPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: iced::mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        // Check if the renderer thread has reported a different viewport scale
        // factor (written by `prepare()` on the previous frame). If so,
        // reconfigure the glyph atlas to rasterize at the correct resolution.
        // The atomic is initialized to 1.0 (not 0) so the first frame is valid.
        let vp_scale = bits_to_f32(self.viewport_scale_factor.load(Ordering::Relaxed));
        if self.glyph_state.borrow_mut().update_scale_factor(vp_scale) {
            log::info!("Retina: reconfigured glyph atlas for scale_factor={}", vp_scale);
        }

        // All vertex positions remain in logical coordinates.
        // iced_wgpu sets the GPU viewport to bounds × scale_factor, so the
        // vertex shader maps logical positions through NDC to physical pixels.
        let screen_w = bounds.width;
        let screen_h = bounds.height;
        // Apply window opacity to background color alpha channel
        let bg_color = [
            self.background_color[0],
            self.background_color[1],
            self.background_color[2],
            self.background_color[3] * self.opacity,
        ];

        // Phase 1: Snapshot terminal state under a short lock
        struct CellSnapshot {
            row: usize,
            col: usize,
            c: char,
            fg: Color,
            bg: Color,
        }

        let (cells, display_offset, cursor_row, cursor_col, colors, selection_range, vi_mode_active, vi_cursor_row, vi_cursor_col) = {
            let term = self.ctx.terminal().lock_unfair();
            let display_offset = term.grid().display_offset();
            let cursor = &term.grid().cursor;
            let cursor_row = (cursor.point.line.0 + display_offset as i32) as usize;
            let cursor_col = cursor.point.column.0;
            let colors = *term.colors();
            let cells: Vec<CellSnapshot> = term.grid().display_iter()
                .map(|indexed| CellSnapshot {
                    row: (indexed.point.line.0 + display_offset as i32) as usize,
                    col: indexed.point.column.0,
                    c: indexed.cell.c,
                    fg: indexed.cell.fg,
                    bg: indexed.cell.bg,
                })
                .collect();
            // 选区高亮:把 Selection 解析为 SelectionRange(可能跨多行/可能为 Block)。
            // 渲染时(下方 cell 循环)用 SelectionRange::contains(point) 判定每格是否高亮。
            let selection_range = term.selection.as_ref().and_then(|s| s.to_range(&*term));
            // GAP-4.1: Vi mode cursor info
            let vi_mode_active = term.mode().contains(TermMode::VI);
            let (vi_cursor_row, vi_cursor_col) = if vi_mode_active {
                let vc = &term.vi_mode_cursor.point;
                let row = (vc.line.0 + display_offset as i32) as usize;
                let col = vc.column.0;
                (row, col)
            } else {
                (0usize, 0usize)
            };
            (cells, display_offset, cursor_row, cursor_col, colors, selection_range, vi_mode_active, vi_cursor_row, vi_cursor_col)
        }; // Lock released here

        // Inject configured foreground color into the colors array so that
        // color_to_f32() can resolve NamedColor::Foreground from config
        // instead of falling back to hardcoded defaults.
        let mut colors = colors;
        let fg_rgb = Rgb {
            r: (self.foreground_color[0] * 255.0) as u8,
            g: (self.foreground_color[1] * 255.0) as u8,
            b: (self.foreground_color[2] * 255.0) as u8,
        };
        colors[NamedColor::Foreground as usize] = Some(fg_rgb);

        // Phase 2: Build GPU primitives from snapshot (no lock held)
        let mut gs = self.glyph_state.borrow_mut();
        let cell_w = gs.cell_width;   // logical
        let cell_h = gs.cell_height;  // logical
        let font_descent = gs.font_descent; // logical

        let computed_cols = (screen_w / cell_w).floor().max(1.0) as usize;
        let computed_rows = (screen_h / cell_h).floor().max(1.0) as usize;
        let pad_x = (screen_w - computed_cols as f32 * cell_w).max(0.0) / 2.0;
        let pad_y = (screen_h - computed_rows as f32 * cell_h).max(0.0) / 2.0;

        let mut bg_vertices = Vec::new();
        let mut bg_indices = Vec::new();
        let mut text_vertices = Vec::new();
        let mut text_indices = Vec::new();

        // Full-screen background
        push_bg_quad(&mut bg_vertices, &mut bg_indices, 0.0, 0.0, screen_w, screen_h, bg_color);

        // 选区高亮颜色(半透明蓝)。
        // alacritty kernel 的 Colors 数组没有 selection 槽,色彩配置不通过它走;
        // 这里固定一个 macOS 系蓝色,后续可暴露为 config 项。
        let sel_color: [f32; 4] = [0.30, 0.50, 0.90, 0.40];

        // GAP-4.1: Vi search match highlighting.
        // Compute visible search matches if a pattern is set.
        let search_match_color: [f32; 4] = [0.85, 0.65, 0.15, 0.35]; // warm amber
        let search_matches: Vec<std::ops::RangeInclusive<TermPoint>> = {
            let pattern = self.vi_search_pattern.lock().ok().and_then(|p| p.clone());
            match pattern {
                Some(pat) if !pat.is_empty() => {
                    let term = self.ctx.terminal().lock();
                    // Cache the built RegexSearch keyed on the pattern string so
                    // we don't rebuild 4 lazy DFAs every frame. The guard is held
                    // for the lifetime of the borrowed regex (RegexIter needs
                    // `&mut RegexSearch`).
                    let mut cache = match self.cached_search.lock() {
                        Ok(g) => g,
                        // Poisoned mutex — recover the inner guard rather than
                        // dropping matches entirely.
                        Err(e) => e.into_inner(),
                    };
                    let stale = cache.as_ref().map_or(true, |(p, _)| *p != pat);
                    if stale {
                        *cache = alacritty_terminal::term::search::RegexSearch::new(&pat)
                            .ok()
                            .map(|r| (pat.clone(), r));
                    }
                    match cache.as_mut() {
                        Some((_, regex)) => {
                            let top = term.topmost_line();
                            let screen = term.screen_lines() as i32;
                            let start = TermPoint::new(top, Column(0));
                            let end = TermPoint::new(
                                Line(-(display_offset as i32) + screen - 1),
                                term.last_column(),
                            );
                            let iter = alacritty_terminal::term::search::RegexIter::new(
                                start,
                                end,
                                alacritty_terminal::index::Direction::Right,
                                &*term,
                                regex,
                            );
                            iter.collect()
                        }
                        None => Vec::new(),
                    }
                }
                _ => Vec::new(),
            }
        };
        // Helper: check if a grid point falls inside any search match
        let is_search_match = |p: TermPoint| -> bool {
            search_matches.iter().any(|m| m.contains(&p))
        };

        // GAP-10: trigger highlight regions (pre-filtered to the visible viewport).
        let trigger_regions: Vec<crate::triggers::HighlightRegion> = {
            let guard = match self.trigger_highlights.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let top = -(display_offset as i32);
            let bottom = -(display_offset as i32) + computed_rows as i32 - 1;
            guard
                .iter()
                .filter(|r| r.line >= top && r.line <= bottom)
                .copied()
                .collect()
        };

        let trigger_folds: Vec<crate::triggers::FoldRegion> = {
            let guard = match self.trigger_folds.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let top = -(display_offset as i32);
            let bottom = -(display_offset as i32) + computed_rows as i32 - 1;
            guard
                .iter()
                .filter(|f| f.collapsed && f.bottom_line >= top && f.top_line <= bottom)
                .copied()
                .collect()
        };

        let fold_conceal_color: [f32; 4] = [0.15, 0.15, 0.18, 0.85];

        // Iterate cell snapshots: backgrounds + text glyphs
        for cell in &cells {
            let x = pad_x + cell.col as f32 * cell_w;
            let y = pad_y + cell.row as f32 * cell_h;

            // Cell background
            if cell.bg != Color::Named(NamedColor::Background) {
                let color = color_to_f32(cell.bg, &colors);
                push_bg_quad(&mut bg_vertices, &mut bg_indices, x, y, cell_w, cell_h, color);
            }

            // 选区高亮(画在 cell 自身背景之上、glyph 之前)。
            // cell.row 是 display 行号(grid_line + display_offset),
            // selection_range 用 grid Line(可能为负),需换算到同一坐标系再判定。
            if let Some(range) = selection_range {
                let grid_line = Line(cell.row as i32 - display_offset as i32);
                let p = TermPoint::new(grid_line, Column(cell.col));
                if range.contains(p) {
                    push_bg_quad(
                        &mut bg_vertices,
                        &mut bg_indices,
                        x, y, cell_w, cell_h,
                        sel_color,
                    );
                }
            }

            // GAP-4.1: Search match highlighting (amber overlay)
            if !search_matches.is_empty() {
                let grid_line = Line(cell.row as i32 - display_offset as i32);
                let p = TermPoint::new(grid_line, Column(cell.col));
                if is_search_match(p) {
                    push_bg_quad(
                        &mut bg_vertices,
                        &mut bg_indices,
                        x, y, cell_w, cell_h,
                        search_match_color,
                    );
                }
            }

            // GAP-10: Trigger match highlighting (per-rule color overlay).
            if !trigger_regions.is_empty() {
                let grid_line = Line(cell.row as i32 - display_offset as i32);
                for region in &trigger_regions {
                    if region.line == grid_line.0
                        && cell.col >= region.col_start
                        && cell.col < region.col_end
                    {
                        push_bg_quad(
                            &mut bg_vertices,
                            &mut bg_indices,
                            x, y, cell_w, cell_h,
                            region.color,
                        );
                        break;
                    }
                }
            }

            let grid_line_i32 = cell.row as i32 - display_offset as i32;
            let fold_concealed = trigger_folds.iter().any(|f| {
                f.collapsed && grid_line_i32 > f.top_line && grid_line_i32 <= f.bottom_line
            });
            if fold_concealed {
                push_bg_quad(
                    &mut bg_vertices,
                    &mut bg_indices,
                    x, y, cell_w, cell_h,
                    fold_conceal_color,
                );
            }

            // Text glyph — skip concealed fold interior rows.
            if fold_concealed {
                continue;
            }

            if cell.c == ' ' || cell.c == '\t' || cell.c == '\0' {
                continue;
            }

            if let Some(glyph) = gs.get_or_rasterize(cell.c) {
                let fg = color_to_f32(cell.fg, &colors);
                let atlas_sel = if glyph.atlas_kind == AtlasKind::Color { 1.0 } else { 0.0 };
                let glyph_color = if glyph.atlas_kind == AtlasKind::Color {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    fg
                };

                // Glyph offsets are already in logical coordinates
                let gx = x + glyph.x_offset;
                let gy = y + (cell_h + font_descent - glyph.y_offset);
                let gw = glyph.width;
                let gh = glyph.height;

                let base = text_vertices.len() as u32;
                text_vertices.extend([
                    TextVertex { position: [gx, gy], tex_coords: [glyph.u, glyph.v], color: glyph_color, atlas_selector: atlas_sel, x_offset: 0.0, y_offset: 0.0 },
                    TextVertex { position: [gx + gw, gy], tex_coords: [glyph.u2, glyph.v], color: glyph_color, atlas_selector: atlas_sel, x_offset: 0.0, y_offset: 0.0 },
                    TextVertex { position: [gx + gw, gy + gh], tex_coords: [glyph.u2, glyph.v2], color: glyph_color, atlas_selector: atlas_sel, x_offset: 0.0, y_offset: 0.0 },
                    TextVertex { position: [gx, gy + gh], tex_coords: [glyph.u, glyph.v2], color: glyph_color, atlas_selector: atlas_sel, x_offset: 0.0, y_offset: 0.0 },
                ]);
                text_indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }

        // Cursor
        let cursor_color = [0.6, 0.6, 0.6, 0.8];
        let cx = pad_x + cursor_col as f32 * cell_w;
        let cy = pad_y + cursor_row as f32 * cell_h;
        push_bg_quad(&mut bg_vertices, &mut bg_indices, cx, cy, cell_w, cell_h, cursor_color);

        // GAP-4.1: Vi mode cursor — teal/cyan to visually distinguish from normal cursor.
        // In vi mode the primary cursor is dimmed and the vi cursor is prominent.
        if vi_mode_active {
            // Dim the primary cursor
            let dim_color = [0.4, 0.4, 0.4, 0.3];
            push_bg_quad(&mut bg_vertices, &mut bg_indices, cx, cy, cell_w, cell_h, dim_color);
            // Render vi cursor in teal
            let vi_color = [0.2, 0.8, 0.7, 0.85];
            let vx = pad_x + vi_cursor_col as f32 * cell_w;
            let vy = pad_y + vi_cursor_row as f32 * cell_h;
            push_bg_quad(&mut bg_vertices, &mut bg_indices, vx, vy, cell_w, cell_h, vi_color);
        }

        // Take dirty atlas data (Fix 2: use per-region upload instead of full
        // Vec clone). Returns Arc<Vec<u8>> + dirty rects; the GPU side loops
        // regions and issues one `queue.write_texture` per rect.
        let grayscale_atlas = gs.atlas.take_dirty_regions();
        let color_atlas = gs.atlas.take_color_dirty_regions();

        drop(gs);

        TerminalPrimitive {
            bg_vertices,
            bg_indices,
            text_vertices,
            text_indices,
            grayscale_atlas,
            color_atlas,
            gpu: Mutex::new(GpuResources::default()),
            viewport_scale_factor: self.viewport_scale_factor.clone(),
            background_image: self.background_image.lock().unwrap().clone(),
            background_mode: self.background_mode,
            background_opacity: self.background_opacity,
            background_signature: self.background_signature,
        }
    }
}

// --- Helpers ---

fn push_bg_quad(v: &mut Vec<BgVertex>, i: &mut Vec<u32>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    let base = v.len() as u32;
    v.extend([
        BgVertex { position: [x, y], color },
        BgVertex { position: [x + w, y], color },
        BgVertex { position: [x + w, y + h], color },
        BgVertex { position: [x, y + h], color },
    ]);
    i.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn color_to_f32(color: Color, colors: &alacritty_terminal::term::color::Colors) -> [f32; 4] {
    match color {
        Color::Named(named) => {
            let rgb = colors[named].unwrap_or(named_color_default(named));
            rgb_to_f32(rgb)
        }
        Color::Spec(rgb) => rgb_to_f32(rgb),
        Color::Indexed(idx) => {
            let rgb = colors[idx as usize].unwrap_or(Rgb { r: 204, g: 204, b: 204 });
            rgb_to_f32(rgb)
        }
    }
}

fn named_color_default(named: NamedColor) -> Rgb {
    match named {
        NamedColor::Black => Rgb { r: 0, g: 0, b: 0 },
        NamedColor::Red => Rgb { r: 204, g: 0, b: 0 },
        NamedColor::Green => Rgb { r: 78, g: 154, b: 6 },
        NamedColor::Yellow => Rgb { r: 196, g: 160, b: 0 },
        NamedColor::Blue => Rgb { r: 52, g: 101, b: 164 },
        NamedColor::Magenta => Rgb { r: 117, g: 80, b: 123 },
        NamedColor::Cyan => Rgb { r: 6, g: 152, b: 154 },
        NamedColor::White => Rgb { r: 211, g: 215, b: 207 },
        NamedColor::BrightBlack => Rgb { r: 85, g: 87, b: 83 },
        NamedColor::BrightRed => Rgb { r: 239, g: 41, b: 41 },
        NamedColor::BrightGreen => Rgb { r: 138, g: 226, b: 52 },
        NamedColor::BrightYellow => Rgb { r: 252, g: 233, b: 79 },
        NamedColor::BrightBlue => Rgb { r: 114, g: 159, b: 207 },
        NamedColor::BrightMagenta => Rgb { r: 173, g: 127, b: 168 },
        NamedColor::BrightCyan => Rgb { r: 52, g: 226, b: 226 },
        NamedColor::BrightWhite => Rgb { r: 238, g: 238, b: 236 },
        NamedColor::Foreground => Rgb { r: 211, g: 215, b: 207 },
        NamedColor::Background => Rgb { r: 28, g: 28, b: 30 },
        NamedColor::Cursor => Rgb { r: 211, g: 215, b: 207 },
        _ => Rgb { r: 204, g: 204, b: 204 },
    }
}

fn rgb_to_f32(rgb: Rgb) -> [f32; 4] {
    [rgb.r as f32 / 255.0, rgb.g as f32 / 255.0, rgb.b as f32 / 255.0, 1.0]
}

#[cfg(test)]
mod shaper_tests {
    //! T-core-shaper v2: tests for the renderer-side shaper wiring.
    //!
    //! Validates that the per-glyph `x_advance` returned by the shaper
    //! actually drives the cursor advance the renderer would apply.
    use super::*;
    use pureterm_alacritty_runtime::shaper::{MonospaceShaper, Shaper};

    #[test]
    fn monospace_shaper_advances_one_cell_per_ascii() {
        let shaper = MonospaceShaper::new();
        let run = shaper.shape("abc", 7.0);
        assert_eq!(run.glyphs.len(), 3);
        for g in &run.glyphs {
            assert_eq!(g.x_advance, 7.0);
        }
    }

    #[test]
    fn monospace_shaper_advances_two_cells_for_cjk() {
        let shaper = MonospaceShaper::new();
        let run = shaper.shape("你好", 7.0);
        assert_eq!(run.glyphs.len(), 2);
        for g in &run.glyphs {
            assert_eq!(g.x_advance, 14.0);
        }
    }

    #[test]
    fn cluster_to_cell_offsets_account_for_wide_chars() {
        let shaper = MonospaceShaper::new();
        let run = shaper.shape("a你b", 7.0);
        // ASCII consumes cell 0, CJK consumes cells 1+2, ASCII consumes cell 3
        assert_eq!(run.cluster_to_cell, vec![0, 1, 3]);
    }
}
