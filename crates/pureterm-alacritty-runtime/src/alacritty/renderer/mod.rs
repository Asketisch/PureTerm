//! wgpu 8-channel rendering pipeline.
//!
//! PureTerm uses wgpu for GPU-accelerated rendering with 8 render passes
//! executed on an independent render thread via crossbeam_channel.

pub mod font_fallback;
pub mod glyph_atlas;
pub mod render_pass;
pub mod render_thread;
pub mod vertex_builders;
