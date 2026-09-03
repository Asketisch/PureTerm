//! `TerminalPrimitive::prepare()` 的拆分:把原本 ~550 行的 `prepare` 函数体
//! 拆成 4 个自由函数(`prepare_bg_color_pass` / `prepare_bg_image_pass` /
//! `prepare_atlas_textures` / `prepare_cell_text_pass`),由
//! [`super::primitive::TerminalPrimitive`] 的 `impl Primitive::prepare` 调用。
//!
//! Rust 不允许把一个 trait impl 的方法拆到多个文件,所以这里把方法体拆成
//! 自由函数(逻辑保持原样,只是把 `self` / `pipeline` / `gpu` 作为参数显式传入)。

use iced::Rectangle;
use iced_wgpu::graphics::Viewport;
use wgpu::util::DeviceExt;

use crate::background_image::{BackgroundMode, DecodedBackground};
use pureterm_alacritty_runtime::renderer::glyph_atlas::AtlasDirty;

use super::pipeline::{GpuResources, TerminalPipeline};
use super::primitive::TerminalPrimitive;
use super::vertices::{BgImageVertex, ScreenUniforms};

/// 1x1 占位像素数据,用于图集尚无脏内容的时刻。
/// 即使尚未光栅化任何字形,也能使绑定组保持有效。
pub(super) const PLACEHOLDER_GRAY: &[u8] = &[0u8];
pub(super) const PLACEHOLDER_COLOR: &[u8] = &[0u8, 0u8, 0u8, 0u8];

/// 构建并上传 BgColor 通道的每帧缓冲(顶点 / 索引 /
/// 统一缓冲 / 绑定组)。无 bg 顶点时为空操作。
pub(super) fn prepare_bg_color_pass(
    prim: &TerminalPrimitive,
    pipeline: &TerminalPipeline,
    device: &wgpu::Device,
    _bounds: &Rectangle,
    gpu: &mut GpuResources,
    uniforms: &ScreenUniforms,
) {
    if !prim.bg_vertices.is_empty() {
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("BG VB"), contents: bytemuck::cast_slice(&prim.bg_vertices), usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("BG IB"), contents: bytemuck::cast_slice(&prim.bg_indices), usage: wgpu::BufferUsages::INDEX,
        });
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("BG UB"), contents: bytemuck::cast_slice(&[*uniforms]), usage: wgpu::BufferUsages::UNIFORM,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BG Bind"), layout: &pipeline.uniform_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
        });
        gpu.bg_vertex_buffer = Some(vb);
        gpu.bg_index_buffer = Some(ib);
        gpu.bg_bind_group = Some(bg);
        gpu.bg_index_count = prim.bg_indices.len() as u32;
    } else {
        gpu.bg_index_count = 0;
    }
}

/// GAP-54:构建并上传背景图通道(顶点 / 索引 /
/// 统一缓冲 / 纹理绑定组)。未安装图片或
/// opacity == 0 时跳过。纹理上传以签名门控,相同的图片
/// 在帧间不会重复上传。
pub(super) fn prepare_bg_image_pass(
    prim: &TerminalPrimitive,
    pipeline: &mut TerminalPipeline,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bounds: &Rectangle,
    gpu: &mut GpuResources,
) {
    // 构建顶点 + 索引 + 统一缓冲 + 纹理绑定组。未安装图片
    // 或 opacity 为 0 时跳过。
    let bg_image: Option<&DecodedBackground> = prim.background_image.as_ref();
    if let (Some(img), true) = (bg_image, prim.background_opacity > 0.0) {
        let mode = prim.background_mode;
        let opacity = prim.background_opacity;
        // 为图片构建屏幕空间四边形。模式:
        //   Center  → 视口子矩形,按图片原始尺寸
        //   Tile    → 平铺视口(uv 通过 Repeat 采样器回绕)
        //   Stretch → 全视口四边形,uv 0..1
        let (vp_w, vp_h) = (bounds.width, bounds.height);
        let (iw, ih) = (img.width as f32, img.height as f32);
        let (qx, qy, qw, qh) = match mode {
            BackgroundMode::Center => {
                // 钳制到视口。
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

        // 统一缓冲(每窗格一份;使用屏幕尺寸 + bg 字段)。
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

        // 纹理:根据签名上传(或跳过)。
        if gpu.bg_image_signature != prim.background_signature {
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
            gpu.bg_image_signature = prim.background_signature;
        }

        gpu.bg_image_vertex_buffer = Some(vb);
        gpu.bg_image_index_buffer = Some(ib);
        gpu.bg_image_uniform_bind_group = Some(ubg);
        gpu.bg_image_index_count = idx.len() as u32;
    } else {
        gpu.bg_image_index_count = 0;
        // 丢弃每帧缓冲但保留纹理(签名仍然有效)。
        gpu.bg_image_vertex_buffer = None;
        gpu.bg_image_index_buffer = None;
        gpu.bg_image_uniform_bind_group = None;
    }
}

/// 同步灰度(R8)+ 彩色(RGBA8)字形图集纹理。
///
/// 每个图集决定:(a) 重建纹理(首帧或尺寸增大)
/// 或 (b) 仅通过 `queue.write_texture` 上传新的脏数据。
/// 仅当纹理真正重建时才重建绑定组,
/// 消除每帧的 GPU 抖动。
///
/// 关键:当稳态帧的 `take_dirty_data()` 返回 None(无新字形)时,
/// 必须保留既有的纹理 / 视图 / 绑定组。1x1 占位纹理
/// 只在纹理为 None 的首帧使用——把它并入每帧尺寸检查,
/// 会导致每个静态帧都把纹理销毁并重建为 1x1,
/// 清空字形缓存,
/// 使文本不可见(Fix 3.5)。
pub(super) fn prepare_atlas_textures(
    prim: &TerminalPrimitive,
    pipeline: &mut TerminalPipeline,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) {
    let mut views_changed = false;

    if let Some(atlas) = &prim.grayscale_atlas {
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
            write_atlas_regions_r8(queue, tex, atlas, w);
        }
    } else if pipeline.grayscale_atlas_texture.is_none() {
        // 首帧尚未光栅化任何字形:创建 1x1 占位纹理,
        // 使绑定组有效。后续无新数据的帧会落入下面的
        // “什么都不做”分支。
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
    // 否则:纹理存在且无新数据 → 保留纹理 / 视图。

    if let Some(atlas) = &prim.color_atlas {
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
            write_atlas_regions_rgba(queue, tex, atlas, w);
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
    // 否则:纹理存在且无新数据 → 保留纹理 / 视图。

    // 仅当视图变化(纹理被重建)或绑定组尚未建立时,
    // 才重建图集绑定组。
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
}

/// 同尺寸、部分上传(Fix 2):每个脏矩形一次 `write_texture`,
/// 字节偏移进入 Arc 共享的图集缓冲。wgpu 要求 bytes_per_row
/// 是 COPY_BYTES_PER_ROW_ALIGNMENT(256)的倍数;图集
/// 分配器输出 2 的幂宽度,因此恒成立。R8Unorm = 1 字节/像素。
fn write_atlas_regions_r8(queue: &wgpu::Queue, tex: &wgpu::Texture, atlas: &AtlasDirty, w: u32) {
    for region in &atlas.regions {
        let byte_offset = region.y as usize * w as usize + region.x as usize;
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
                bytes_per_row: Some(w), // R8Unorm = 每像素 1 字节
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

/// RGBA = 每像素 4 字节;bytes_per_row 必须是 256 的倍数
/// (图集宽度为 2 的幂,因此恒成立)。
fn write_atlas_regions_rgba(queue: &wgpu::Queue, tex: &wgpu::Texture, atlas: &AtlasDirty, w: u32) {
    for region in &atlas.regions {
        let byte_offset = (region.y as usize * w as usize + region.x as usize) * 4;
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
                bytes_per_row: Some(w * 4), // Rgba8UnormSrgb = 每像素 4 字节
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

/// 构建并上传 CellText 通道的每帧缓冲(顶点 / 索引 /
/// 统一缓冲 / 绑定组)。无文本顶点时为空操作。
pub(super) fn prepare_cell_text_pass(
    prim: &TerminalPrimitive,
    pipeline: &TerminalPipeline,
    device: &wgpu::Device,
    gpu: &mut GpuResources,
    uniforms: &ScreenUniforms,
) {
    if !prim.text_vertices.is_empty() {
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Text VB"), contents: bytemuck::cast_slice(&prim.text_vertices), usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Text IB"), contents: bytemuck::cast_slice(&prim.text_indices), usage: wgpu::BufferUsages::INDEX,
        });
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Text UB"), contents: bytemuck::cast_slice(&[*uniforms]), usage: wgpu::BufferUsages::UNIFORM,
        });
        gpu.text_uniform_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Text Uniform BG"), layout: &pipeline.text_uniform_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
        }));
        gpu.text_vertex_buffer = Some(vb);
        gpu.text_index_buffer = Some(ib);
        gpu.text_index_count = prim.text_indices.len() as u32;
    } else {
        gpu.text_index_count = 0;
    }
}

// 当本模块在没有下游调用方引用 `Viewport` 的情况下编译
// (例如 feature 门控路径)时,抑制未使用导入的警告。
#[allow(dead_code)]
fn _viewport_use_hint(_vp: &Viewport) {}
