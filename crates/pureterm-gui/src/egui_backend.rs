//! egui-wgpu backend for PureTerm settings overlay.

use std::sync::Arc;
use winit::event::WindowEvent;
use winit::window::Window;

/// CJK font family names to search for (per platform).
#[cfg(target_os = "macos")]
const CJK_FAMILIES: &[&str] = &[
    "PingFang SC", "PingFang TC", "PingFang HK",
    "Hiragino Sans GB", "STHeiti", "Songti SC",
];
#[cfg(target_os = "linux")]
const CJK_FAMILIES: &[&str] = &[
    "Noto Sans CJK SC", "Noto Sans CJK TC",
    "WenQuanYi Micro Hei", "Source Han Sans",
];
#[cfg(target_os = "windows")]
const CJK_FAMILIES: &[&str] = &[
    "Microsoft YaHei", "SimHei",
    "Source Han Sans SC", "Noto Sans CJK SC",
];
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
const CJK_FAMILIES: &[&str] = &[];

/// Manages egui rendering within the wgpu pipeline.
pub struct EguiBackend {
    pub context: egui::Context,
    pub winit_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
}

/// Configure egui to use system CJK fonts for proper rendering of Chinese/Japanese/Korean text.
fn configure_cjk_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    // Platform-specific CJK font families, in priority order.
    let cjk_families: &[&str] = if cfg!(target_os = "macos") {
        &["PingFang SC", "PingFang TC", "Hiragino Sans GB"]
    } else if cfg!(target_os = "linux") {
        &["Noto Sans CJK SC", "Noto Sans CJK TC", "WenQuanYi Micro Hei"]
    } else {
        &["Microsoft YaHei", "SimHei"]
    };

    for family_name in cjk_families {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family_name)],
            weight: fontdb::Weight::default(),
            stretch: fontdb::Stretch::default(),
            style: fontdb::Style::default(),
        };

        if let Some(face_id) = db.query(&query) {
            if let Some((source, _index)) = db.face_source(face_id) {
                if let fontdb::Source::File(path) = source {
                    if let Ok(data) = std::fs::read(&path) {
                        let font_name = format!("{}-fallback", family_name);
                        fonts.font_data.insert(
                            font_name.clone().into(),
                            std::sync::Arc::new(egui::FontData::from_owned(data)),
                        );

                        // Add as fallback for both Proportional and Monospace families
                        for family in &[egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                            if let Some(fallbacks) = fonts.families.get_mut(family) {
                                fallbacks.push(font_name.clone().into());
                            }
                        }
                        log::info!("Loaded CJK font '{}' for egui settings panel", family_name);
                        break; // Use the first successfully loaded CJK font
                    }
                }
            }
        }
    }

    ctx.set_fonts(fonts);
}

impl EguiBackend {
    /// Create a new egui backend.
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        window: Arc<Window>,
    ) -> Self {
        let egui_ctx = egui::Context::default();

        // Configure CJK fonts for proper display of Chinese text in settings panel
        configure_cjk_fonts(&egui_ctx);

        let viewport_id = egui_ctx.viewport_id();

        let winit_state = egui_winit::State::new(
            egui_ctx.clone(),
            viewport_id,
            &*window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        let renderer = egui_wgpu::Renderer::new(device, surface_format, egui_wgpu::RendererOptions::default());

        Self {
            context: egui_ctx,
            winit_state,
            renderer,
        }
    }

    /// Register CJK fonts with the egui context so Chinese/Japanese/Korean
    /// text can be rendered in the settings panel, modals, etc.
    pub fn setup_cjk_fonts(&self) {
        let cjk_data = Self::find_cjk_font();
        match cjk_data {
            Some(data) => {
                let mut fonts = egui::FontDefinitions::default();

                // Add the CJK font data
                fonts.font_data.insert(
                    "cjk".to_owned(),
                    Arc::new(egui::FontData::from_owned(data)),
                );

                // Append CJK font to Proportional and Monospace families
                // egui automatically tries each font in order for each character
                let cjk_name = "cjk".to_owned();
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    if let Some(fallback) = fonts.families.get_mut(&family) {
                        fallback.push(cjk_name.clone());
                    }
                }

                self.context.set_fonts(fonts);
                log::info!("egui: CJK fonts registered successfully");
            }
            None => {
                log::warn!("egui: no CJK font found on system, Chinese text may not render");
            }
        }
    }

    /// Find a suitable CJK font on the system.
    fn find_cjk_font() -> Option<Vec<u8>> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        for family_name in CJK_FAMILIES {
            let face_id = db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(family_name)],
                weight: fontdb::Weight::default(),
                stretch: fontdb::Stretch::default(),
                style: fontdb::Style::default(),
            });

            if let Some(id) = face_id {
                if let Some((source, _index)) = db.face_source(id) {
                    if let fontdb::Source::File(path) = source {
                        if let Ok(data) = std::fs::read(&path) {
                            log::info!("egui: found CJK font: {} at {:?}", family_name, path);
                            return Some(data);
                        }
                    }
                }
            }
        }

        None
    }

    /// Process a winit window event through egui.
    /// Returns true if the event was consumed by egui.
    pub fn on_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        let response = self.winit_state.on_window_event(window, event);
        response.consumed
    }

    /// Run the egui update and get full output.
    #[allow(deprecated)]
    pub fn run(
        &self,
        raw_input: egui::RawInput,
        update_fn: impl FnMut(&egui::Context),
    ) -> egui::FullOutput {
        self.context.run(raw_input, update_fn)
    }

    /// Render egui paint jobs onto an existing surface texture view.
    /// Uses LoadOp::Load to draw on top of existing content.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_view: &wgpu::TextureView,
        paint_jobs: Vec<egui::ClippedPrimitive>,
        textures_delta: egui::TexturesDelta,
        screen_descriptor: egui_wgpu::ScreenDescriptor,
    ) {
        // Upload egui textures
        for (id, image_delta) in &textures_delta.set {
            self.renderer.update_texture(device, queue, *id, image_delta);
        }

        // Create encoder and upload vertex/index buffers before render
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("egui Buffers Encoder"),
        });
        self.renderer.update_buffers(device, queue, &mut encoder, &paint_jobs, &screen_descriptor);

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut render_pass = render_pass.forget_lifetime();

            self.renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        queue.submit(std::iter::once(encoder.finish()));

        // Free textures that were removed
        for id in &textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}
