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
    // 共享渲染管线(创建后不可变)
    pub(super) bg_color_pipeline: wgpu::RenderPipeline,
    pub(super) text_pipeline: wgpu::RenderPipeline,

    /// 设备句柄 —— 供 [`LazyPipeline`] 在首次真正需要时创建管线。
    /// `wgpu::Device` 内部是 `Arc`,克隆很廉价。
    pub(super) device: wgpu::Device,
    /// 管线目标格式(与创建时一致)。
    pub(super) format: wgpu::TextureFormat,

    // 共享的绑定组布局(bind group layout)
    pub(super) uniform_bgl: wgpu::BindGroupLayout,
    pub(super) text_atlas_bgl: wgpu::BindGroupLayout,
    pub(super) text_uniform_bgl: wgpu::BindGroupLayout,

    // 共享采样器
    pub(super) nearest_sampler: wgpu::Sampler,

    // 共享图集纹理(GlyphState 在所有窗格间共享;
    // 首个调用 prepare() 的窗格负责上传脏图集数据)。
    // 纹理跨帧持久;只有图集尺寸变化时才重建
    // (例如新字形迫使图集扩容时)。
    // 稳态帧上使用 `queue.write_texture` 做局部更新。
    pub(super) grayscale_atlas_texture: Option<wgpu::Texture>,
    pub(super) color_atlas_texture: Option<wgpu::Texture>,
    pub(super) grayscale_atlas_size: (u32, u32),
    pub(super) color_atlas_size: (u32, u32),
    // 由上述纹理派生的视图;绑定组被缓存,
    // 仅当视图变化(即纹理重建)时才重建。
    pub(super) grayscale_atlas_view: Option<wgpu::TextureView>,
    pub(super) color_atlas_view: Option<wgpu::TextureView>,
    pub(super) text_atlas_bind_group: Option<wgpu::BindGroup>,

    // GAP-54:背景图像管线 + 绑定组布局。
    //
    // 管线本身**按需创建**(见 [`LazyPipeline`]):Metal 上每创建一个
    // shader module 都要跑一次 `newLibraryWithSource:`,实测约
    // 25-45ms。背景图是可选特性,大多数会话根本不用,没必要让
    // 每次启动都先付这份编译代价。
    pub(super) bg_image_pipeline: LazyPipeline,
    pub(super) bg_image_bgl: wgpu::BindGroupLayout,
    pub(super) bg_image_sampler: wgpu::Sampler,

    // T-core-graphics v3:图像四边形(image quad)管线 + 图集绑定。
    //
    // 图集是单张 2048×2048 的 RGBA8 纹理(与 CPU 侧的 `ImageAtlas` 对应);
    // 所有可见的 Sixel / 分块图形 / 内联图像都位于其中,
    // 由 `ImageAtlas` 的行步长分配器打包放置。
    // CPU 每帧上传脏子矩形;本结构持有
    // 常驻的 GPU 纹理 + 视图 + 绑定组,避免无新内容时
    // 重复重建。管线同样按需创建(见 [`LazyPipeline`]。
    pub(super) image_quad_pipeline: LazyPipeline,
    pub(super) image_atlas_bgl: wgpu::BindGroupLayout,
    pub(super) image_atlas_sampler: wgpu::Sampler,
    pub(super) image_atlas_texture: Option<wgpu::Texture>,
    pub(super) image_atlas_view: Option<wgpu::TextureView>,
    pub(super) image_atlas_size: (u32, u32),
    pub(super) image_atlas_bind_group: Option<wgpu::BindGroup>,
}

/// 按需创建的渲染管线。
///
/// 背景图与图像四边形(Sixel)两条通道在绝大多数会话里根本不会产生
/// 绘制调用,但它们各自的 WGSL module 在 Metal 上要花 25-45ms 编译
/// (实测:每次 `create_shader_module` 都会触发一次
/// `-[MTLDevice newLibraryWithSource:]`)。把管线推迟到第一次真正
/// 需要时创建,启动就能省掉这笔开销;真用到时(例如用户配置了背景图)
/// 再付一次,渲染语义完全不变。
///
/// 用 `OnceLock` 而非 `Mutex<Option<…>>`:创建只发生一次,快路径
/// 只是一次原子读。
pub struct LazyPipeline {
    inner: std::sync::OnceLock<wgpu::RenderPipeline>,
    factory: Box<dyn Fn(&wgpu::Device, wgpu::TextureFormat) -> wgpu::RenderPipeline + Send + Sync>,
}

impl LazyPipeline {
    /// 创建一个惰性管线。`factory` 在首次 [`LazyPipeline::get`] 时被调用一次。
    pub fn new<F>(factory: F) -> Self
    where
        F: Fn(&wgpu::Device, wgpu::TextureFormat) -> wgpu::RenderPipeline + Send + Sync + 'static,
    {
        Self { inner: std::sync::OnceLock::new(), factory: Box::new(factory) }
    }

    /// 取管线;尚未创建则当场创建。
    ///
    /// 首次调用会阻塞当前线程(通常是渲染线程)直到 shader 编译完成,
    /// 这与「启动时一次性编译」相比只是把成本挪到了真正需要的那一刻。
    pub fn get(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> &wgpu::RenderPipeline {
        if let Some(existing) = self.inner.get() {
            return existing;
        }
        let pipeline = (self.factory)(device, format);
        self.inner.get_or_init(|| pipeline)
    }
}

impl fmt::Debug for LazyPipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyPipeline").field("created", &self.inner.get().is_some()).finish()
    }
}

/// 每个 `TerminalPrimitive` 实例拥有的逐帧 GPU 资源。
/// 用 `Mutex` 存储,因为 `Primitive::prepare()` 接收 `&self` 且必须 `Sync`。
pub(in crate::terminal_shader) struct GpuResources {
    pub(in crate::terminal_shader) bg_vertex_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) bg_index_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) bg_bind_group: Option<wgpu::BindGroup>,
    pub(in crate::terminal_shader) bg_index_count: u32,
    /// bg 通道里「全屏底色 quad」的索引条数;其余是单元格底色等。
    pub(in crate::terminal_shader) bg_base_index_count: u32,

    pub(in crate::terminal_shader) text_vertex_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) text_index_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) text_uniform_bind_group: Option<wgpu::BindGroup>,
    pub(in crate::terminal_shader) text_index_count: u32,

    // GAP-54:每个窗格的背景图像状态。
    pub(in crate::terminal_shader) bg_image_vertex_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) bg_image_index_buffer: Option<wgpu::Buffer>,
    pub(in crate::terminal_shader) bg_image_index_count: u32,
    pub(in crate::terminal_shader) bg_image_bind_group: Option<wgpu::BindGroup>,
    pub(in crate::terminal_shader) bg_image_uniform_bind_group: Option<wgpu::BindGroup>,
    pub(in crate::terminal_shader) bg_image_texture: Option<wgpu::Texture>,
    pub(in crate::terminal_shader) bg_image_view: Option<wgpu::TextureView>,
    /// 已加载图像字节的哈希 —— 用于在图像跨帧未变时
    /// 跳过 GPU 重新上传。
    pub(in crate::terminal_shader) bg_image_signature: u64,

    // T-core-graphics v3:每个窗格的图像四边形状态。
    // 顶点 / 索引缓冲每帧根据
    // `TerminalPrimitive::image_quad_draws` 重建;绑定组引用
    // 共享的管线级图集纹理(见 `image_atlas_bind_group`)。
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
            bg_base_index_count: 0,
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
        crate::startup_trace::mark_delta("gpu_pipeline_enter");
        // --- 共享的 uniform 绑定组布局 ---
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

        // --- BgColor 管线 ---
        let bg_shader_src = include_str!(
            "../../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/bg_color.wgsl"
        );
        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("BgColor Shader"),
            source: wgpu::ShaderSource::Wgsl(bg_shader_src.into()),
        });
        crate::startup_trace::mark_delta("wgsl_bg_color");
        let bg_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("BgColor Layout"),
            bind_group_layouts: &[&uniform_bgl],
            push_constant_ranges: &[],
        });
        let bg_color_pipeline = create_pipeline(device, "BgColor", &bg_shader, &bg_layout, BgVertex::desc(), format);

        // --- CellText 管线 ---
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
        crate::startup_trace::mark_delta("wgsl_cell_text");
        let text_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("CellText Layout"),
            bind_group_layouts: &[&text_atlas_bgl, &text_uniform_bgl],
            push_constant_ranges: &[],
        });
        let text_pipeline = create_pipeline(device, "CellText", &text_shader, &text_layout, TextVertex::desc(), format);

        // --- GAP-54:背景图像管线 ---
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
        // 背景图管线按需创建(见 `LazyPipeline`)。
        let bg_image_shader_src: &'static str = include_str!(
            "../../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/bg_image.wgsl"
        );
        let bg_image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("BgImage Layout"),
            bind_group_layouts: &[&uniform_bgl, &bg_image_bgl],
            push_constant_ranges: &[],
        });
        let bg_image_pipeline = {
            let layout = bg_image_layout.clone();
            LazyPipeline::new(move |device, format| {
                crate::startup_trace::mark_delta("wgsl_bg_image_lazy");
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("BgImage Shader"),
                    source: wgpu::ShaderSource::Wgsl(bg_image_shader_src.into()),
                });
                create_pipeline(
                    device,
                    "BgImage",
                    &shader,
                    &layout,
                    BgImageVertex::desc(),
                    format,
                )
            })
        };

        // --- T-core-graphics v3:图像四边形管线 ---
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
        // 图像四边形(Sixel)管线按需创建(见 `LazyPipeline`)。
        let image_quad_shader_src: &'static str = include_str!(
            "../../../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/image_quad.wgsl"
        );
        let image_quad_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ImageQuad Layout"),
            bind_group_layouts: &[&uniform_bgl, &image_atlas_bgl],
            push_constant_ranges: &[],
        });
        let image_quad_pipeline = {
            let layout = image_quad_layout.clone();
            LazyPipeline::new(move |device, format| {
                crate::startup_trace::mark_delta("wgsl_image_quad_lazy");
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("ImageQuad Shader"),
                    source: wgpu::ShaderSource::Wgsl(image_quad_shader_src.into()),
                });
                create_pipeline(
                    device,
                    "ImageQuad",
                    &shader,
                    &layout,
                    ImageQuadVertex::desc(),
                    format,
                )
            })
        };

        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Text Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        crate::startup_trace::mark_delta("gpu_pipelines_built");
        Self {
            bg_color_pipeline,
            device: device.clone(),
            format,
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
