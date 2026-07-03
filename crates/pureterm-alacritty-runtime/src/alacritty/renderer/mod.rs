//! wgpu 字体图集 — 由主路径 `crates/pureterm/src/terminal_shader.rs` 引用。
//!
//! - `glyph_atlas` — grayscale + RGBA text glyph atlas (LRU + dirty region upload).
//! - `image_atlas` — RGBA-only image atlas for Sixel / Kitty / iTerm2 inline.

pub mod glyph_atlas;
/// T-core-graphics v2: RGBA image atlas for Sixel / Kitty / iTerm2.
pub mod image_atlas;
