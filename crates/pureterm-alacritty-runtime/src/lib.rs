//! Alacritty 内核 + GPU 字体图集运行时(精简版)。
//!
//! 本 crate 仅保留 `renderer::glyph_atlas` 模块,提供 GPU 字体图集的
//! LRU + dirty region 上传 API,被主路径 `crates/pureterm/src/terminal_shader.rs`
//! 直接引用。
//!
//! 设计约束(CLAUDE.md "维护规则"):
//! - vendored Alacritty 源码不做无关格式化或重构,便于同步上游
//! - 新功能应在主路径(`crates/pureterm/src/`)实现

#![warn(rust_2018_idioms, future_incompatible)]

#[path = "alacritty/renderer/mod.rs"]
pub mod renderer;
