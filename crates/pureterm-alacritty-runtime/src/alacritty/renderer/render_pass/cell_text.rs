//! 单元格文本渲染通道。
//!
//! 双图集渲染：灰度图集 (R8) 用于文本字形，彩色图集 (RGBA) 用于 emoji。
//! 每个顶点携带 atlas_selector 字段，片段着色器据此选择采样哪个纹理。

use bytemuck::{Pod, Zeroable};
use wgpu::{Device, Queue, RenderPipeline, SurfaceConfiguration};

use super::{
    create_nearest_sampler, create_simple_pipeline, create_uniform_bgl,
    ensure_index_buffer, ensure_vertex_buffer, index_buffer_size, vertex_buffer_size, PassData,
    RenderPass, ScreenUniforms,
};

/// 文本字形四边形的顶点：位置 + 纹理坐标 + RGBA 颜色 + 图集选择器 + 字形偏移。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TextVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
    /// 0.0 = 灰度图集, 1.0 = 彩色图集.
    pub atlas_selector: f32,
    /// 字形 X 偏移（组合字符定位）.
    pub x_offset: f32,
    /// 字形 Y 偏移（组合字符定位）.
    pub y_offset: f32,
}

impl TextVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 6] = [
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
            offset: (std::mem::size_of::<[f32; 2]>() * 2) as u64,
            shader_location: 2,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: (std::mem::size_of::<[f32; 2]>() * 2 + std::mem::size_of::<[f32; 4]>()) as u64,
            shader_location: 3,
            format: wgpu::VertexFormat::Float32,
        },
        wgpu::VertexAttribute {
            offset: (std::mem::size_of::<[f32; 2]>() * 2 + std::mem::size_of::<[f32; 4]>() + std::mem::size_of::<f32>()) as u64,
            shader_location: 4,
            format: wgpu::VertexFormat::Float32,
        },
        wgpu::VertexAttribute {
            offset: (std::mem::size_of::<[f32; 2]>() * 2 + std::mem::size_of::<[f32; 4]>() + std::mem::size_of::<f32>() * 2) as u64,
            shader_location: 5,
            format: wgpu::VertexFormat::Float32,
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

/// 单元格文本通道的初始四边形容量（10K 顶点 = 2500 个四边形）。
const TEXT_INITIAL_QUADS: u64 = 10_000 / 4;

/// 创建双图集绑定组布局（灰度纹理+采样器 + 彩色纹理+采样器 = 4 bindings）。
fn create_dual_atlas_bgl(device: &Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("CellText Dual Atlas BGL"),
        entries: &[
            // Binding 0: 灰度图集纹理
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
            // Binding 1: 灰度采样器
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // Binding 2: 彩色图集纹理
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
            // Binding 3: 彩色采样器
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// 创建 1x1 透明占位纹理视图。
fn create_placeholder_texture(
    device: &Device,
    queue: &Queue,
    label: &str,
    format: wgpu::TextureFormat,
    pixel: &[u8],
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes_per_row = pixel.len() as u32;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixel,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// 单元格文本渲染通道。
pub struct CellTextPass {
    pipeline: RenderPipeline,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    atlas_bind_group: Option<wgpu::BindGroup>,
    atlas_bgl: wgpu::BindGroupLayout,
    /// PureTerm B3.4: track whether the atlas bind group is stale and
    /// needs a rebuild. Without this flag, every dirty frame would call
    /// `device.create_bind_group` (expensive).
    atlas_bind_group_dirty: bool,
    sampler: wgpu::Sampler,
    /// 灰度图集纹理视图（在 render thread 中设置）。
    grayscale_atlas_view: Option<wgpu::TextureView>,
    /// 彩色图集纹理视图。
    color_atlas_view: Option<wgpu::TextureView>,
    vertex_cap: u64,
    index_cap: u64,
    index_count: u32,
}

impl CellTextPass {
    /// 创建新的单元格文本通道。
    pub fn new(
        device: &Device,
        queue: &Queue,
        config: &SurfaceConfiguration,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let uniform_bgl = create_uniform_bgl(
            device,
            "CellText Uniform BGL",
            wgpu::ShaderStages::VERTEX_FRAGMENT,
        );
        let atlas_bgl = create_dual_atlas_bgl(device);

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CellText Uniform Buffer"),
            size: std::mem::size_of::<ScreenUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("CellText Uniform BG"),
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

        let sampler = create_nearest_sampler(device, "CellText Sampler");

        let pipeline = create_simple_pipeline(
            device,
            "CellText Pipeline",
            include_str!("../shaders/cell_text.wgsl"),
            TextVertex::desc(),
            &[&atlas_bgl, &uniform_bgl],
            surface_format,
        );

        let initial_cap = TEXT_INITIAL_QUADS;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CellText Vertex Buffer"),
            size: vertex_buffer_size::<TextVertex>(initial_cap),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CellText Index Buffer"),
            size: index_buffer_size(initial_cap),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 初始化 uniforms
        let uniforms = ScreenUniforms::new(0.0, config.width as f32, config.height as f32);
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        Self {
            pipeline,
            vertex_buffer: Some(vertex_buffer),
            index_buffer: Some(index_buffer),
            uniform_buffer,
            bind_group: uniform_bg,
            atlas_bind_group: None,
            atlas_bgl,
            atlas_bind_group_dirty: true, // PureTerm B3.4: rebuild on first frame.
            sampler,
            grayscale_atlas_view: None,
            color_atlas_view: None,
            vertex_cap: initial_cap,
            index_cap: initial_cap,
            index_count: 0,
        }
    }

    /// 设置灰度图集纹理视图（仅灰度，用于占位或非 emoji 场景）。
    pub fn set_atlas_texture(&mut self, device: &Device, atlas_view: &wgpu::TextureView) {
        // Clone the view — wgpu::TextureView is Arc-backed, cheap to clone.
        self.grayscale_atlas_view = Some(atlas_view.clone());
        self.rebuild_atlas_bind_group(device);
    }

    /// 设置双图集纹理视图（灰度 + 彩色）。
    pub fn set_atlas_textures(
        &mut self,
        device: &Device,
        gray_view: &wgpu::TextureView,
        color_view: &wgpu::TextureView,
    ) {
        self.grayscale_atlas_view = Some(gray_view.clone());
        self.color_atlas_view = Some(color_view.clone());
        self.rebuild_atlas_bind_group(device);
    }

    /// 仅更新灰度图集视图（保留彩色图集不变）。
    pub fn update_grayscale_atlas(&mut self, device: &Device, gray_view: &wgpu::TextureView) {
        // PureTerm B3.4: only mark dirty if the view actually changed.
        let changed = self.grayscale_atlas_view.as_ref() != Some(gray_view);
        self.grayscale_atlas_view = Some(gray_view.clone());
        if changed {
            self.atlas_bind_group_dirty = true;
        }
    }

    /// 仅更新彩色图集视图（保留灰度图集不变）。
    pub fn update_color_atlas(&mut self, device: &Device, color_view: &wgpu::TextureView) {
        let changed = self.color_atlas_view.as_ref() != Some(color_view);
        self.color_atlas_view = Some(color_view.clone());
        if changed {
            self.atlas_bind_group_dirty = true;
        }
    }

    /// 重建图集绑定组。
    fn rebuild_atlas_bind_group(&mut self, device: &Device) {
        // PureTerm B3.4: skip the expensive `create_bind_group` when the
        // underlying texture views haven't changed.
        if !self.atlas_bind_group_dirty && self.atlas_bind_group.is_some() {
            return;
        }
        self.atlas_bind_group_dirty = false;
        // 如果没有彩色图集视图，使用占位纹理。
        let color_view = match &self.color_atlas_view {
            Some(v) => v,
            None => {
                // 创建 1x1 透明 RGBA 占位纹理并缓存
                // 注意：这里每次都创建新的不是最优的，但只在首次调用时发生
                return;
            },
        };

        let gray_view = match &self.grayscale_atlas_view {
            Some(v) => v,
            None => return,
        };

        self.atlas_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("CellText Dual Atlas BG"),
            layout: &self.atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(gray_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
    }

    /// 初始化彩色图集占位纹理。在首次上传彩色图集前调用。
    pub fn ensure_color_placeholder(&mut self, device: &Device, queue: &Queue) {
        if self.color_atlas_view.is_some() {
            return;
        }
        let placeholder = create_placeholder_texture(
            device,
            queue,
            "Color Atlas Placeholder",
            wgpu::TextureFormat::Rgba8UnormSrgb,
            &[0, 0, 0, 0],
        );
        self.color_atlas_view = Some(placeholder);
        self.rebuild_atlas_bind_group(device);
    }

    /// 更新此帧的顶点/索引数据和 uniforms。
    pub fn update(
        &mut self,
        device: &Device,
        queue: &Queue,
        vertices: &[TextVertex],
        indices: &[u32],
        data: &PassData,
    ) {
        self.index_count = indices.len() as u32;

        // 计算需要的四边形数
        let needed_quads = ((vertices.len() as u64) + 3) / 4;
        ensure_vertex_buffer::<TextVertex>(
            device,
            "CellText Vertex Buffer",
            &mut self.vertex_buffer,
            &mut self.vertex_cap,
            needed_quads,
        );
        ensure_index_buffer(
            device,
            "CellText Index Buffer",
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

        let uniforms =
            ScreenUniforms::new(data.corner_radius, data.screen_width, data.screen_height);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }
}

impl RenderPass for CellTextPass {
    fn label(&self) -> &'static str {
        "CellText"
    }

    fn update_buffers(&mut self, _device: &Device, _queue: &Queue, _data: &PassData) {
        // 缓冲区更新在 update() 中处理
    }

    fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.index_count == 0 {
            return;
        }
        let Some(ref atlas_bg) = self.atlas_bind_group else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, atlas_bg, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        if let Some(ref vb) = self.vertex_buffer {
            pass.set_vertex_buffer(0, vb.slice(..));
        }
        if let Some(ref ib) = self.index_buffer {
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        }
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // 动态缓冲区在 update 时按需增长，resize 无需特殊处理
    }
}
