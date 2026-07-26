//! PureTerm GPU 终端渲染核心(Iced 0.14 shader)。
//!
//! 本模块原本是单文件 2000+ 行,按 CLAUDE.md「文件行数(强制)」(警戒线 500 /
//! 红线 800)拆为子目录,各子文件按职责切分:
//!
//! - [`vertices`] — 4 个 GPU 顶点类型 + `ScreenUniforms`。
//! - [`pipeline`] — `TerminalPipeline`(iced_wgpu `Pipeline`)+ `GpuResources`。
//! - [`primitive`] — `TerminalPrimitive`(iced_wgpu `Primitive`)外壳 + `draw`。
//! - [`primitive_prepare`] — `TerminalPrimitive::prepare` 的拆分函数
//!   (bg_color / bg_image / atlas / cell_text pass)。
//! - [`primitive_quad_pass`] — T-core-graphics v3 image quad pass 的
//!   `prepare` 逻辑。
//! - [`glyph`] — `GlyphState`(字形光栅化 + 字形 atlas)。
//! - [`image_quad`] — `ImageQuadState` + `ImageQuadDraw`。
//! - [`program`] — `TerminalProgram`(iced `shader::Program`) + `TerminalState`。
//! - [`helpers`] — 小工具函数。
//! - [`shaper_tests`] — 单元测试。
//!
//! 拆分原则:不修改任何函数体逻辑,仅做物理搬运 + 必要的可见性升级
//! (`pub(super)` / `pub(in crate::terminal_shader)`)。所有原本 `pub` 的类型
//! 在此根重新 `pub use` re-export,保持外部引用不变。

mod glyph;
mod helpers;
mod image_quad;
mod pipeline;
mod primitive;
mod primitive_prepare;
mod primitive_quad_pass;
mod program;
mod vertices;

#[cfg(test)]
mod shaper_tests;

pub use glyph::GlyphState;
pub use image_quad::{ImageQuadDraw, ImageQuadState};
pub use primitive::TerminalPrimitive;
pub use program::{TerminalProgram, TerminalState};
pub use vertices::{BgImageVertex, BgVertex, ImageQuadVertex, ScreenUniforms, TextVertex};
