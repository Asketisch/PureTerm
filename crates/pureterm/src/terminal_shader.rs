use bytemuck::{Pod, Zeroable};
use iced::widget::shader::{self, Action};
use iced::Rectangle;
use iced_wgpu::graphics::Viewport;
use iced_wgpu::primitive::{self, Pipeline, Primitive};
use wgpu::util::DeviceExt;
use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use crate::terminal_context::TerminalContext;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use crossfont::{FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Style, Weight, Slant};
use pureterm_alacritty_runtime::renderer::glyph_atlas::{AtlasKind, CachedGlyph, WgpuGlyphAtlas};

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
    pub _padding: f32,
}

// --- TerminalPipeline ---

pub struct TerminalPipeline {
    // BgColor pass
    bg_color_pipeline: wgpu::RenderPipeline,
    uniform_bgl: wgpu::BindGroupLayout,
    bg_vertex_buffer: Option<wgpu::Buffer>,
    bg_index_buffer: Option<wgpu::Buffer>,
    bg_bind_group: Option<wgpu::BindGroup>,
    bg_index_count: u32,

    // CellText pass
    text_pipeline: wgpu::RenderPipeline,
    text_atlas_bgl: wgpu::BindGroupLayout,
    text_uniform_bgl: wgpu::BindGroupLayout,
    nearest_sampler: wgpu::Sampler,
    grayscale_atlas_view: Option<wgpu::TextureView>,
    color_atlas_view: Option<wgpu::TextureView>,
    text_vertex_buffer: Option<wgpu::Buffer>,
    text_index_buffer: Option<wgpu::Buffer>,
    text_atlas_bind_group: Option<wgpu::BindGroup>,
    text_uniform_bind_group: Option<wgpu::BindGroup>,
    text_index_count: u32,
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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

        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Text Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Self {
            bg_color_pipeline,
            uniform_bgl,
            bg_vertex_buffer: None,
            bg_index_buffer: None,
            bg_bind_group: None,
            bg_index_count: 0,
            text_pipeline,
            text_atlas_bgl,
            text_uniform_bgl,
            nearest_sampler,
            grayscale_atlas_view: None,
            color_atlas_view: None,
            text_vertex_buffer: None,
            text_index_buffer: None,
            text_atlas_bind_group: None,
            text_uniform_bind_group: None,
            text_index_count: 0,
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
    grayscale_atlas: Option<(Vec<u8>, u32, u32)>,
    color_atlas: Option<(Vec<u8>, u32, u32)>,
}

impl fmt::Debug for TerminalPrimitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalPrimitive")
            .field("bg_verts", &self.bg_vertices.len())
            .field("text_verts", &self.text_vertices.len())
            .finish()
    }
}

impl Primitive for TerminalPrimitive {
    type Pipeline = TerminalPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        let uniforms = ScreenUniforms {
            corner_radius: 0.0,
            screen_width: bounds.width,
            screen_height: bounds.height,
            _padding: 0.0,
        };

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
            pipeline.bg_vertex_buffer = Some(vb);
            pipeline.bg_index_buffer = Some(ib);
            pipeline.bg_bind_group = Some(bg);
            pipeline.bg_index_count = self.bg_indices.len() as u32;
        } else {
            pipeline.bg_index_count = 0;
        }

        // --- Atlas textures ---
        if let Some((ref data, w, h)) = self.grayscale_atlas {
            let tex = device.create_texture_with_data(_queue, &wgpu::TextureDescriptor {
                label: Some("Gray Atlas"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }, wgpu::util::TextureDataOrder::LayerMajor, data);
            pipeline.grayscale_atlas_view = Some(tex.create_view(&Default::default()));
        }

        if let Some((ref data, w, h)) = self.color_atlas {
            let tex = device.create_texture_with_data(_queue, &wgpu::TextureDescriptor {
                label: Some("Color Atlas"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }, wgpu::util::TextureDataOrder::LayerMajor, data);
            pipeline.color_atlas_view = Some(tex.create_view(&Default::default()));
        }

        // Ensure placeholder atlas views if not yet set
        if pipeline.grayscale_atlas_view.is_none() {
            let tex = device.create_texture_with_data(_queue, &wgpu::TextureDescriptor {
                label: Some("Placeholder Gray"), size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
            }, wgpu::util::TextureDataOrder::LayerMajor, &[0u8]);
            pipeline.grayscale_atlas_view = Some(tex.create_view(&Default::default()));
        }
        if pipeline.color_atlas_view.is_none() {
            let tex = device.create_texture_with_data(_queue, &wgpu::TextureDescriptor {
                label: Some("Placeholder Color"), size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
            }, wgpu::util::TextureDataOrder::LayerMajor, &[0, 0, 0, 0]);
            pipeline.color_atlas_view = Some(tex.create_view(&Default::default()));
        }

        // Rebuild atlas bind group
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
            pipeline.text_uniform_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Text Uniform BG"), layout: &pipeline.text_uniform_bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
            }));
            pipeline.text_vertex_buffer = Some(vb);
            pipeline.text_index_buffer = Some(ib);
            pipeline.text_index_count = self.text_indices.len() as u32;
        } else {
            pipeline.text_index_count = 0;
        }
    }

    fn draw(
        &self,
        pipeline: &Self::Pipeline,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        // Draw bg_color pass
        if pipeline.bg_index_count > 0 {
            if let (Some(vb), Some(ib), Some(bg)) = (&pipeline.bg_vertex_buffer, &pipeline.bg_index_buffer, &pipeline.bg_bind_group) {
                render_pass.set_pipeline(&pipeline.bg_color_pipeline);
                render_pass.set_bind_group(0, bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..pipeline.bg_index_count, 0, 0..1);
            }
        }

        // Draw cell_text pass
        if pipeline.text_index_count > 0 {
            if let (Some(vb), Some(ib), Some(atlas_bg), Some(uniform_bg)) = (
                &pipeline.text_vertex_buffer, &pipeline.text_index_buffer,
                &pipeline.text_atlas_bind_group, &pipeline.text_uniform_bind_group,
            ) {
                render_pass.set_pipeline(&pipeline.text_pipeline);
                render_pass.set_bind_group(0, atlas_bg, &[]);
                render_pass.set_bind_group(1, uniform_bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..pipeline.text_index_count, 0, 0..1);
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
    pub cell_width: f32,
    pub cell_height: f32,
    pub font_descent: f32,
}

impl GlyphState {
    pub fn new() -> Self {
        let mut rasterizer = Rasterizer::new().expect("Failed to create rasterizer");
        let font_size = Size::new(14.0);
        let font_desc = FontDesc::new("monospace", Style::Description { slant: Slant::Normal, weight: Weight::Normal });
        let font_key = rasterizer.load_font(&font_desc, font_size).expect("Failed to load font");
        let metrics = rasterizer.metrics(font_key, font_size).expect("Failed to get metrics");

        let cell_width = metrics.average_advance.floor().max(1.0) as f32;
        let cell_height = metrics.line_height.floor().max(1.0) as f32;
        let font_descent = metrics.descent as f32;

        Self {
            atlas: WgpuGlyphAtlas::new(),
            rasterizer,
            font_key,
            font_size,
            cell_width,
            cell_height,
            font_descent,
        }
    }

    pub fn get_or_rasterize(&mut self, c: char) -> Option<CachedGlyph> {
        if let Some(cached) = self.atlas.get(0, c) {
            return Some(cached);
        }

        let glyph_key = GlyphKey { character: c, font_key: self.font_key, size: self.font_size };
        match self.rasterizer.get_glyph(glyph_key) {
            Ok(glyph) => Some(self.atlas.insert(0, c, &glyph)),
            Err(_) => None,
        }
    }
}

// --- TerminalProgram ---

pub struct TerminalProgram {
    pub ctx: Arc<TerminalContext>,
    pub glyph_state: Arc<RefCell<GlyphState>>,
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
        let screen_w = bounds.width;
        let screen_h = bounds.height;
        let bg_color = [0.11, 0.11, 0.12, 1.0];

        let term = self.ctx.terminal.lock();
        let colors = term.colors();
        let display_offset = term.grid().display_offset();
        let cursor = &term.grid().cursor;

        let mut gs = self.glyph_state.borrow_mut();
        let cell_w = gs.cell_width;
        let cell_h = gs.cell_height;
        let font_descent = gs.font_descent;

        // Use the terminal's actual grid dimensions (set by resize)
        let cols = term.columns();
        let rows = term.screen_lines();
        let pad_x = (screen_w - cols as f32 * cell_w).max(0.0) / 2.0;
        let pad_y = (screen_h - rows as f32 * cell_h).max(0.0) / 2.0;

        let mut bg_vertices = Vec::new();
        let mut bg_indices = Vec::new();
        let mut text_vertices = Vec::new();
        let mut text_indices = Vec::new();

        // Full-screen background
        push_bg_quad(&mut bg_vertices, &mut bg_indices, 0.0, 0.0, screen_w, screen_h, bg_color);

        // Iterate cells: backgrounds + text glyphs
        for indexed in term.grid().display_iter() {
            let point = indexed.point;
            let visual_row = (point.line.0 + display_offset as i32) as usize;
            let visual_col = point.column.0;
            let x = pad_x + visual_col as f32 * cell_w;
            let y = pad_y + visual_row as f32 * cell_h;

            // Cell background
            let cell_bg = indexed.cell.bg;
            if cell_bg != Color::Named(NamedColor::Background) {
                let color = color_to_f32(cell_bg, colors);
                push_bg_quad(&mut bg_vertices, &mut bg_indices, x, y, cell_w, cell_h, color);
            }

            // Text glyph
            let c = indexed.cell.c;
            if c == ' ' || c == '\t' || c == '\0' {
                continue;
            }

            if let Some(glyph) = gs.get_or_rasterize(c) {
                let fg = color_to_f32(indexed.cell.fg, colors);
                let atlas_sel = if glyph.atlas_kind == AtlasKind::Color { 1.0 } else { 0.0 };
                let glyph_color = if glyph.atlas_kind == AtlasKind::Color {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    fg
                };

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
        let cursor_visual_row = (cursor.point.line.0 + display_offset as i32) as usize;
        let cursor_visual_col = cursor.point.column.0;
        let cursor_color = [0.6, 0.6, 0.6, 0.8];
        let cx = pad_x + cursor_visual_col as f32 * cell_w;
        let cy = pad_y + cursor_visual_row as f32 * cell_h;
        push_bg_quad(&mut bg_vertices, &mut bg_indices, cx, cy, cell_w, cell_h, cursor_color);

        // Take dirty atlas data
        let grayscale_atlas = gs.atlas.take_dirty_data();
        let color_atlas = gs.atlas.take_color_dirty_data();

        drop(gs);
        drop(term);

        TerminalPrimitive {
            bg_vertices,
            bg_indices,
            text_vertices,
            text_indices,
            grayscale_atlas,
            color_atlas,
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
