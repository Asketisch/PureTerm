//! T-core-graphics v3: image quad pass 的 `prepare()` 逻辑,拆自
//! `TerminalPrimitive::prepare` 的 Step A/B/C。
//!
//! - Step A:排空解码通道,把新解码的 RGBA 位图塞进 CPU 侧 `ImageQuadState.atlas`。
//! - Step B:把脏区上载到 GPU atlas 纹理(整张重建 or 增量 `write_texture`)。
//! - Step C:根据 `image_quad_draws` 构建 per-frame 顶点/索引/Uniform 缓冲。
//!
//! 详见 [`super::primitive::TerminalPrimitive`] 与拆分前的
//! `terminal_shader.rs` Step A/B/C 注释。

use wgpu::util::DeviceExt;

use pureterm_alacritty_runtime::renderer::image_atlas::AtlasDirty as ImageAtlasDirty;

use super::image_quad::ImageQuadDraw;
use super::pipeline::{GpuResources, TerminalPipeline};
use super::primitive::TerminalPrimitive;
use super::vertices::{ImageQuadVertex, ScreenUniforms};

/// Step A: drain the decode channel. Each `DecodedImage` carries a freshly
/// decoded RGBA bitmap + the original placement; we push it into
/// `ImageQuadState.atlas` and rely on its dirty-region tracker to drive the
/// GPU upload below.
pub(super) fn drain_decode_channel(prim: &TerminalPrimitive) {
    {
        let mut rx = prim.decode_rx.lock().unwrap();
        while let Ok(decoded) = rx.try_recv() {
            let mut state = prim.image_quad_state.lock().unwrap();
            // `insert` returns `None` when the image exceeds ATLAS_SIZE
            // or has wrong byte length — silently skip (logged at the
            // store layer).
            let _ = state.insert(
                decoded.id as u64,
                std::sync::Arc::new(decoded.rgba),
                decoded.width,
                decoded.height,
            );
        }
    }
}

/// Step B: upload dirty atlas regions to the GPU texture. Mirrors the
/// gray/color atlas upload pattern (recreate vs partial upload vs first-frame
/// placeholder) but for the image atlas (RGBA8, 2048×2048).
pub(super) fn prepare_image_atlas_texture(
    prim: &TerminalPrimitive,
    pipeline: &mut TerminalPipeline,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) {
    let mut image_atlas_views_changed = false;
    let mut image_atlas_dirty: Option<ImageAtlasDirty> = None;
    {
        let mut state = prim.image_quad_state.lock().unwrap();
        image_atlas_dirty = state.take_dirty_data();
    }
    if let Some(atlas) = &image_atlas_dirty {
        let (w, h) = (atlas.width, atlas.height);
        let need_recreate = pipeline.image_atlas_texture.is_none()
            || pipeline.image_atlas_size != (w, h);
        if need_recreate {
            let tex = device.create_texture_with_data(queue, &wgpu::TextureDescriptor {
                label: Some("Image Atlas"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }, wgpu::util::TextureDataOrder::LayerMajor, &atlas.data);
            pipeline.image_atlas_texture = Some(tex);
            pipeline.image_atlas_size = (w, h);
            pipeline.image_atlas_view = Some(
                pipeline.image_atlas_texture.as_ref().unwrap().create_view(&Default::default())
            );
            image_atlas_views_changed = true;
        } else if let Some(tex) = pipeline.image_atlas_texture.as_ref() {
            // Steady-state partial upload: one write_texture per dirty rect.
            // `ATLAS_SIZE = 2048` is a multiple of 256 so bytes_per_row = w*4
            // is COPY_BYTES_PER_ROW_ALIGNMENT-safe.
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
                        bytes_per_row: Some(w * 4),
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
    } else if pipeline.image_atlas_texture.is_none() {
        // First frame, no images yet — 1×1 transparent placeholder so the
        // bind group is valid. Mirrors the gray/color atlas first-frame
        // pattern (terminal_shader.rs:737-755).
        let tex = device.create_texture_with_data(queue, &wgpu::TextureDescriptor {
            label: Some("Image Atlas Placeholder"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        }, wgpu::util::TextureDataOrder::LayerMajor, &[0u8; 4]);
        pipeline.image_atlas_texture = Some(tex);
        pipeline.image_atlas_size = (1, 1);
        pipeline.image_atlas_view = Some(
            pipeline.image_atlas_texture.as_ref().unwrap().create_view(&Default::default())
        );
        image_atlas_views_changed = true;
    }
    // Else: texture exists and no new data → preserve texture/view.

    // Recreate the atlas bind group only when views changed (texture was
    // recreated) or the bind group hasn't been built yet.
    if image_atlas_views_changed || pipeline.image_atlas_bind_group.is_none() {
        let view = pipeline.image_atlas_view.as_ref().unwrap();
        pipeline.image_atlas_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Image Atlas BG"),
            layout: &pipeline.image_atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&pipeline.image_atlas_sampler) },
            ],
        }));
    }
}

/// Step C: build per-frame vertex/index buffers from `image_quad_draws`.
/// Sort by z so newer images overlay older ones (matches `ImageStore::insert`
/// monotonic z counter). Cell dimensions come from `prim.cell_w/cell_h`
/// cached at construction time.
pub(super) fn prepare_image_quad_buffers(
    prim: &TerminalPrimitive,
    pipeline: &TerminalPipeline,
    device: &wgpu::Device,
    gpu: &mut GpuResources,
    uniforms: &ScreenUniforms,
) {
    let draws_snapshot: Vec<ImageQuadDraw> = {
        let draws = prim.image_quad_draws.lock().unwrap();
        let mut v = draws.clone();
        v.sort_by_key(|d| d.z);
        v
    };

    if !draws_snapshot.is_empty() {
        let mut verts: Vec<ImageQuadVertex> = Vec::with_capacity(draws_snapshot.len() * 4);
        let mut indices: Vec<u32> = Vec::with_capacity(draws_snapshot.len() * 6);
        let mut valid_draws = 0u32;
        // Cell dimensions: pull from `glyph_state` (logical px).
        // The primitive holds `gpu` mutex; cell dims come from a thread-local
        // we cached at construction time — see construction site below.
        let (cell_w, cell_h) = (prim.cell_w, prim.cell_h);
        {
            let mut state = prim.image_quad_state.lock().unwrap();
            for d in &draws_snapshot {
                // Look up the atlas UV for this image id. Skips images
                // that have been evicted from the atlas (LRU) or whose
                // id no longer exists.
                let Some(placement) = state.get(d.image_id as u64) else {
                    continue;
                };
                let qx = d.col as f32 * cell_w;
                let qy = d.row as f32 * cell_h;
                let qw = d.cols as f32 * cell_w;
                let qh = d.rows as f32 * cell_h;
                let base = verts.len() as u32;
                verts.push(ImageQuadVertex { position: [qx, qy], uv: [placement.u, placement.v] });
                verts.push(ImageQuadVertex { position: [qx + qw, qy], uv: [placement.u2, placement.v] });
                verts.push(ImageQuadVertex { position: [qx + qw, qy + qh], uv: [placement.u2, placement.v2] });
                verts.push(ImageQuadVertex { position: [qx, qy + qh], uv: [placement.u, placement.v2] });
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
                valid_draws += 1;
            }
        }
        if valid_draws > 0 {
            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ImageQuad VB"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ImageQuad IB"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ImageQuad UB"),
                contents: bytemuck::cast_slice(&[*uniforms]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            gpu.image_quad_uniform_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ImageQuad Uniform BG"),
                layout: &pipeline.uniform_bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
            }));
            gpu.image_quad_vertex_buffer = Some(vb);
            gpu.image_quad_index_buffer = Some(ib);
            gpu.image_quad_index_count = indices.len() as u32;
        } else {
            gpu.image_quad_vertex_buffer = None;
            gpu.image_quad_index_buffer = None;
            gpu.image_quad_uniform_bind_group = None;
            gpu.image_quad_index_count = 0;
        }
    } else {
        gpu.image_quad_vertex_buffer = None;
        gpu.image_quad_index_buffer = None;
        gpu.image_quad_uniform_bind_group = None;
        gpu.image_quad_index_count = 0;
    }
}
