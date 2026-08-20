//! wgpu 字体图集 — 由主路径 `crates/pureterm/src/terminal_shader.rs` 引用。
//!
//! - `glyph_atlas`——灰度 + RGBA 文本字形图集(LRU + 脏区域上传)。
//! - `image_atlas`——Sixel / 分块图形 / 内联图像的纯 RGBA 图像图集。

pub mod glyph_atlas;
/// T-core-graphics v2:Sixel / 分块图形 / 内联图像的 RGBA 图像图集。
pub mod image_atlas;
