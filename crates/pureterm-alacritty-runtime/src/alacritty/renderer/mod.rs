//! wgpu 字体图集 — 由主路径 `crates/pureterm/src/terminal_shader.rs` 引用。
//!
//! 历史: 早期 PureTerm 走 Alacritty Display 8-pass 渲染管线,该管线的
//! 所有子模块(bg_color / cell_text / cursor / decorations / post_process
//! / render_thread / harfbuzz_shaper / font_fallback / shape_request /
//! vertex_builders / shaders / bg_image_loader)已迁至
//! `archive/pureterm-display-path/alacritty/renderer/`,主路径不再使用。
//! 仅 `glyph_atlas` 留下,提供 GPU 字体图集的 LRU + dirty region 上传 API
//! (Phase B-1 优化点)。

pub mod glyph_atlas;
