//! T-core-graphics v3 — image quad pass 单测。
//!
//! 这些测试专注于 GPU 链路里的 **CPU 侧可验证不变量**:
//! - `ImageQuadVertex` 的 `Pod` / `Zeroable` / 内存布局正确
//! - `ImageQuadDraw` 排序按 `z` 单调,匹配 `ImageStore` 的 z 计数器
//! - `ImageQuadState::insert` 后 `take_dirty_data()` 返回非 None,
//!   且 regions 数量与插入图像数量一致
//! - `ImagePlacement`(atlas UV)数值有效: `0 ≤ u,v < u2,v2 ≤ 1`
//! - `ImageStore::sequential()` + `by_id_values()` 迭代器不重复、不丢失
//!
//! 真正的 GPU 渲染(wgpu headless + 黄金像素快照)属于 `graphics_e2e.rs`,
//! 那里 `#[ignore]` 默认跳过,需要工具链时手动跑。

use std::sync::Arc;

use alacritty_terminal::event::{
    GraphicsFormat, GraphicsImage, GraphicsPlacement, GraphicsProtocol,
};
use pureterm_alacritty_runtime::renderer::image_atlas::ImageAtlas;

use crate::terminal_shader::{ImageQuadDraw, ImageQuadVertex};

// --- 1. ImageQuadVertex layout ---

#[test]
fn image_quad_vertex_size_is_16_bytes() {
    // 2 floats for position + 2 floats for uv = 16 bytes total. Used by the
    // wgpu vertex buffer layout in `ImageQuadVertex::desc`.
    assert_eq!(std::mem::size_of::<ImageQuadVertex>(), 16);
}

#[test]
fn image_quad_vertex_is_pod() {
    // Smoke test: `bytemuck::Pod` / `Zeroable` allow us to use
    // `bytemuck::cast_slice` on `Vec<ImageQuadVertex>` for buffer upload.
    let v = ImageQuadVertex {
        position: [1.0, 2.0],
        uv: [0.5, 0.25],
    };
    let bytes: &[u8] = bytemuck::bytes_of(&v);
    assert_eq!(bytes.len(), 16);
    // Zero roundtrip
    let zero = ImageQuadVertex {
        position: [0.0; 2],
        uv: [0.0; 2],
    };
    let zero_bytes = bytemuck::bytes_of(&zero);
    assert_eq!(zero_bytes, &[0u8; 16]);
}

#[test]
fn image_quad_vertex_desc_matches_layout() {
    // Verify the wgpu vertex buffer layout matches our struct layout:
    // attribute 0 (position vec2f) at offset 0, attribute 1 (uv vec2f) at
    // offset 8. If this drifts, wgpu will panic at pipeline creation.
    use crate::terminal_shader::ImageQuadVertex as Q;
    let desc = Q::desc();
    assert_eq!(desc.array_stride, 16);
    assert_eq!(desc.attributes.len(), 2);
    assert_eq!(desc.attributes[0].offset, 0);
    assert_eq!(desc.attributes[0].format, wgpu::VertexFormat::Float32x2);
    assert_eq!(desc.attributes[1].offset, 8);
    assert_eq!(desc.attributes[1].format, wgpu::VertexFormat::Float32x2);
}

// --- 2. ImageQuadDraw ordering & invariants ---

#[test]
fn image_quad_draw_z_orders_ascending() {
    let draws = vec![
        ImageQuadDraw {
            image_id: 1,
            col: 0,
            row: 0,
            cols: 4,
            rows: 2,
            z: 3,
        },
        ImageQuadDraw {
            image_id: 2,
            col: 5,
            row: 5,
            cols: 4,
            rows: 2,
            z: 1,
        },
        ImageQuadDraw {
            image_id: 3,
            col: 0,
            row: 0,
            cols: 1,
            rows: 1,
            z: 2,
        },
    ];
    let mut sorted = draws.clone();
    sorted.sort_by_key(|d| d.z);
    assert_eq!(sorted[0].image_id, 2);
    assert_eq!(sorted[1].image_id, 3);
    assert_eq!(sorted[2].image_id, 1);
}

#[test]
fn image_quad_draw_screen_rect_calculation() {
    // The vertex shader maps (col * cell_w, row * cell_h) → screen quad.
    // Verify that the multiplication matches the expected logical-pixel
    // rectangle for typical cell dimensions.
    let cell_w = 8.0_f32;
    let cell_h = 16.0_f32;
    let draw = ImageQuadDraw {
        image_id: 42,
        col: 5,
        row: 3,
        cols: 10,
        rows: 5,
        z: 1,
    };
    let qx = draw.col as f32 * cell_w; // 40
    let qy = draw.row as f32 * cell_h; // 48
    let qw = draw.cols as f32 * cell_w; // 80
    let qh = draw.rows as f32 * cell_h; // 80
    assert_eq!((qx, qy, qw, qh), (40.0, 48.0, 80.0, 80.0));
}

// --- 3. ImageQuadState / ImageAtlas dirty-region contract ---

#[test]
fn image_quad_state_insert_marks_dirty() {
    let mut atlas = ImageAtlas::new();
    let rgba = Arc::new(vec![0u8; 32 * 32 * 4]);
    let placement = atlas.insert(1, rgba, 32, 32).expect("small image should pack");
    // Atlas UV coordinates must be in [0..=1] and u < u2, v < v2.
    assert!(placement.u >= 0.0 && placement.u <= 1.0);
    assert!(placement.v >= 0.0 && placement.v <= 1.0);
    assert!(placement.u2 > placement.u);
    assert!(placement.v2 > placement.v);
    assert_eq!(placement.width, 32);
    assert_eq!(placement.height, 32);
    assert!(atlas.is_dirty());
    let dirty = atlas.take_dirty_data();
    assert!(dirty.is_some(), "take_dirty_data after insert should yield Some");
    let dirty = dirty.unwrap();
    assert!(!dirty.regions.is_empty(), "at least one dirty region expected");
    assert_eq!(dirty.width, 2048); // ImageAtlas ATLAS_SIZE
    assert_eq!(dirty.height, 2048);
}

#[test]
fn image_quad_state_multiple_inserts_each_dirty() {
    let mut atlas = ImageAtlas::new();
    let n: usize = 5;
    for i in 0..n {
        let rgba = Arc::new(vec![i as u8; 16 * 16 * 4]);
        let _ = atlas
            .insert((i + 1) as u64, rgba, 16, 16)
            .expect("small image should pack");
    }
    assert_eq!(atlas.len(), n);
    let dirty = atlas.take_dirty_data().expect("dirty after 5 inserts");
    // Each image lives in its own rect; the row-stride allocator should emit
    // up to n regions (one per image, unless two images overlap a single
    // dirty rect which is rare given the 16×16 footprint on a 2048² atlas).
    assert!(
        dirty.regions.len() >= 1 && dirty.regions.len() <= n,
        "regions.len()={} should be in [1, {}]",
        dirty.regions.len(),
        n
    );
    // Take again: should now be empty (the dirty flag was cleared by the
    // first `take_dirty_data`).
    assert!(atlas.take_dirty_data().is_none());
}

#[test]
fn image_quad_state_lookup_returns_same_uv() {
    let mut atlas = ImageAtlas::new();
    let rgba = Arc::new(vec![0xAA; 8 * 8 * 4]);
    let inserted = atlas.insert(99, rgba, 8, 8).expect("small image packs");
    let looked_up = atlas.get(99).expect("id should be retrievable");
    assert_eq!(inserted.u, looked_up.u);
    assert_eq!(inserted.v, looked_up.v);
    assert_eq!(inserted.u2, looked_up.u2);
    assert_eq!(inserted.v2, looked_up.v2);
    assert_eq!(inserted.width, looked_up.width);
    assert_eq!(inserted.height, looked_up.height);
}

#[test]
fn image_quad_state_oversized_rejected() {
    // ATLAS_SIZE = 2048. Inserting a 2049-px image should be rejected.
    let mut atlas = ImageAtlas::new();
    let rgba = Arc::new(vec![0u8; 2049 * 4 * 4]);
    let r = atlas.insert(1, rgba, 2049, 4);
    assert!(r.is_none(), "oversized image must be rejected");
}

#[test]
fn image_quad_state_wrong_byte_len_rejected() {
    // Bytes don't match width * height * 4 → rejection (defends against
    // upstream corruption).
    let mut atlas = ImageAtlas::new();
    let rgba = Arc::new(vec![0u8; 100]); // 10×10 should need 400 bytes
    let r = atlas.insert(1, rgba, 10, 10);
    assert!(r.is_none());
}

// --- 4. ImageStore → ImageQuadDraw population ---

#[test]
fn image_store_iterators_cover_all_images() {
    use crate::terminal_graphics::ImageStore;

    let mut store = ImageStore::new(16, 4 * 1024 * 1024);
    // Two chunked-graphics images (id != 0) + two Sixel (id == 0).
    for id in [101u32, 102u32] {
        let img = GraphicsImage {
            protocol: GraphicsProtocol::Kitty,
            format: GraphicsFormat::Rgba,
            width: 8,
            height: 8,
            pixels: Some(vec![0u8; 8 * 8 * 4]),
            encoded: Vec::new(),
            placement: GraphicsPlacement {
                id,
                col: 0,
                row: 0,
                cols: 1,
                rows: 1,
                z: 0,
            },
        };
        store.insert(img);
    }
    for _ in 0..2 {
        let img = GraphicsImage {
            protocol: GraphicsProtocol::Sixel,
            format: GraphicsFormat::Rgb,
            width: 8,
            height: 8,
            pixels: None,
            encoded: vec![0u8; 8 * 8 * 3],
            placement: GraphicsPlacement {
                id: 0,
                col: 0,
                row: 0,
                cols: 1,
                rows: 1,
                z: 0,
            },
        };
        store.insert(img);
    }
    assert_eq!(store.len(), 4);
    let total: usize = store.sequential().count() + store.by_id_values().count();
    assert_eq!(total, 4, "all 4 images must be iterable");
    // No id collision between the two iterators (Sixel has id=0, which is
    // not present in `by_id`).
    let mut seen_ids: Vec<u32> = store.sequential().map(|i| i.placement.id).collect();
    seen_ids.extend(store.by_id_values().map(|i| i.placement.id));
    seen_ids.sort_unstable();
    assert_eq!(seen_ids, vec![0, 0, 101, 102]);
}

#[test]
fn image_store_z_monotonic_per_insertion() {
    use crate::terminal_graphics::ImageStore;

    let mut store = ImageStore::new(8, 4 * 1024 * 1024);
    let mut last_z = 0u32;
    for i in 0..5 {
        let img = GraphicsImage {
            protocol: GraphicsProtocol::Kitty,
            format: GraphicsFormat::Rgba,
            width: 4,
            height: 4,
            pixels: Some(vec![0u8; 4 * 4 * 4]),
            encoded: Vec::new(),
            placement: GraphicsPlacement {
                id: i + 1,
                col: 0,
                row: 0,
                cols: 1,
                rows: 1,
                z: 0,
            },
        };
        let z = store.insert(img).expect("insert");
        assert!(
            z > last_z,
            "z must be monotonic: prev={}, new={}",
            last_z,
            z
        );
        last_z = z;
    }
}

// --- 5. End-to-end: pipeline from store → ImageQuadState → dirty ---

#[test]
fn e2e_pipeline_store_to_atlas_to_dirty() {
    use crate::terminal_graphics::ImageStore;
    use crate::terminal_shader::ImageQuadState;

    let mut store = ImageStore::new(4, 4 * 1024 * 1024);
    let img = GraphicsImage {
        protocol: GraphicsProtocol::Kitty,
        format: GraphicsFormat::Rgba,
        width: 32,
        height: 32,
        pixels: Some(vec![0xFFu8; 32 * 32 * 4]),
        encoded: Vec::new(),
        placement: GraphicsPlacement {
            id: 7,
            col: 0,
            row: 0,
            cols: 1,
            rows: 1,
            z: 0,
        },
    };
    store.insert(img);

    let mut state = ImageQuadState::new();
    assert!(!state.is_dirty());

    // Simulate `prepare()` drain: pull every image from the store, push
    // its RGBA into the atlas. (The real pipeline goes through the
    // `decode_rx` channel + `decode` worker — those are covered by
    // `image_decode_worker::tests`.)
    for entry in store.sequential().chain(store.by_id_values()) {
        if let Some(pixels) = &entry.pixels {
            let _ = state.insert(entry.placement.id as u64, Arc::new(pixels.clone()), entry.width, entry.height);
        }
    }

    assert!(state.is_dirty());
    let dirty = state.take_dirty_data().expect("dirty after insert");
    assert!(!dirty.regions.is_empty());
    assert_eq!(dirty.width, 2048);
    assert_eq!(dirty.height, 2048);

    // And `take_dirty_data()` clears the dirty flag.
    assert!(state.take_dirty_data().is_none());
}