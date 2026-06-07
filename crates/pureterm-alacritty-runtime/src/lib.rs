//! Alacritty 内核 + GPU 字体图集运行时(精简版)。
//!
//! 历史: 本 crate 早期暴露 `run_from_env()` 作为 Alacritty 风格入口,
//! 2026 年切换到 Iced 0.14 后主二进制(`crates/pureterm/`)改走
//! `iced_app::run()`,`run_from_env()` 及 vendored Display 路径的所有
//! 子模块(cli / config / daemon / display / event / input / window_context
//! / ssh / trigger / shell_integration / render_pass / render_thread /
//! harfbuzz_shaper / font_fallback / ...) 已迁至
//! `archive/pureterm-display-path/`,主路径仅保留 `renderer::glyph_atlas`。
//!
//! 主二进制唯一的引用:`crates/pureterm/src/terminal_shader.rs:16`
//!   `use pureterm_alacritty_runtime::renderer::glyph_atlas::{...}`
//!
//! 设计约束(CLAUDE.md "维护规则"):
//! - vendored Alacritty 源码不做无关格式化或重构,便于同步上游
//! - 新功能应在主路径(`crates/pureterm/src/`)实现

#![warn(rust_2018_idioms, future_incompatible)]

#[path = "alacritty/renderer/mod.rs"]
pub mod renderer;
