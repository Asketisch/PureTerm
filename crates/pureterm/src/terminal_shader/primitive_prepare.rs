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

/// 1x1 placeholder pixel data used when an atlas has no dirty content yet.
/// Keeps the bind group valid even before the first glyph is rasterized.
pub(super) const PLACEHOLDER_GRAY: &[u8] = &[0u8];
pub(super) const PLACEHOLDER_COLOR: &[u8] = &[0u8, 0u8, 0u8, 0u8];

/// Build + upload the BgColor pass per-frame buffers (vertices / indices /
/// uniform / bind group). No-op when there are no bg vertices.
pub(super) fn prepare_bg_color_pass(
    prim: &TerminalPrimitive,
    pipeline: &TerminalPipeline,
    device: &wgpu::Device,
    bounds: &Rectangle,
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

/// GAP-54: build + upload the background image pass (vertices / indices /
/// uniforms / texture bind group). Skipped when no image installed or
/// opacity == 0. Texture upload is signature-gated so identical images
/// across frames don't re-upload.
pub(super) fn prepare_bg_image_pass(
    prim: &TerminalPrimitive,
    pipeline: &mut TerminalPipeline,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bounds: &Rectangle,
    gpu: &mut GpuResources,
) {
    // Build vertices + indices + uniform + texture bind group. Skip
    // when no image is installed or opacity is 0.
    let bg_image: Option<&DecodedBackground> = prim.background_image.as_ref();
    if let (Some(img), true) = (bg_image, prim.background_opacity > 0.0) {
        let mode = prim.background_mode;
        let opacity = prim.background_opacity;
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
        // Drop per-frame buffers but keep the texture (signature still valid).
        gpu.bg_image_vertex_buffer = None;
        gpu.bg_image_index_buffer = None;
        gpu.bg_image_uniform_bind_group = None;
    }
}

/// Synchronize the grayscale (R8) + color (RGBA8) glyph atlas textures.
///
/// Per atlas, decide whether to (a) recreate the texture (first frame or
/// size grew) or (b) just upload the new dirty data via `queue.write_texture`.
/// The bind group is only rebuilt when a texture is actually recreated,
/// eliminating per-frame GPU churn.
///
/// CRITICAL: when `take_dirty_data()` returns None on a steady-state frame
/// (no new glyphs), the existing texture/view/bind_group must be preserved.
/// The 1x1 placeholder is only used on the very first frame when the texture
/// is None — folding it into a per-frame size check would cause the texture
/// to be destroyed and re-created as 1x1 on every static frame, wiping the
/// glyph cache and rendering text invisible (Fix 3.5).
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
}

/// Same size, partial upload (Fix 2): one `write_texture` per dirty rect,
/// byte-offset into the Arc-shared atlas buffer. Wgpu requires bytes_per_row
/// to be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT (256); the atlas
/// allocator emits power-of-two widths so this holds. R8Unorm = 1 byte/px.
fn write_atlas_regions_r8(queue: &wgpu::Queue, tex: &wgpu::Texture, atlas: &AtlasDirty, w: u32) {
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

/// RGBA = 4 bytes per pixel; bytes_per_row must be a multiple of 256
/// (atlas width is a power of two so this holds).
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

/// Build + upload the CellText pass per-frame buffers (vertices / indices /
/// uniform / bind group). No-op when there are no text vertices.
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

// Suppress unused-import warning when this module is compiled without any
// downstream caller referencing `Viewport` (e.g. feature-gated paths).
#[allow(dead_code)]
fn _viewport_use_hint(_vp: &Viewport) {}
