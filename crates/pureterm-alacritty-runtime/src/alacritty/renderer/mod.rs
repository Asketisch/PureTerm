//! wgpu 字体图集 — 由主路径 `crates/pureterm/src/terminal_shader.rs` 引用。
//!
//! 仅 `glyph_atlas` 留下,提供 GPU 字体图集的 LRU + dirty region 上传 API。

pub mod glyph_atlas;
