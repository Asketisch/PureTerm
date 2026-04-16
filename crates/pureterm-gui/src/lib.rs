pub mod background;
pub mod egui_backend;
pub mod egui_int;
pub mod font_fallback;
pub mod fonts;
pub mod math;
pub mod pipeline;
pub mod render_pass;
pub mod renderer;
pub mod render_thread;
pub mod shaping;
pub mod shader_toy;
pub mod sprite;
pub mod transparency;

pub use renderer::TerminalRenderer;
pub use egui_int::TerminalWidget;
pub use egui_backend::EguiBackend;
