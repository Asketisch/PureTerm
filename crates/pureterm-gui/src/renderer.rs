use crate::fonts::GlyphAtlas;
use crate::pipeline::{TextPipeline, Vertex};
use pureterm_core::theme::ColorScheme;
use pureterm_terminal::ScreenSnapshot;

/// A viewport rectangle for sub-region rendering.
#[derive(Debug, Clone, Copy)]
pub struct ViewportRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Main terminal renderer using wgpu.
pub struct TerminalRenderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    atlas: GlyphAtlas,
    text_pipeline: Option<TextPipeline>,
    atlas_texture: Option<wgpu::Texture>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
}

impl TerminalRenderer {
    /// Create a new terminal renderer.
    pub async fn new() -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("Failed to find GPU adapter"))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("PureTerm GPU Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            }, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create GPU device: {:?}", e))?;

        let mut atlas = GlyphAtlas::new(14.0);
        atlas.load_default_font().map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            atlas,
            text_pipeline: None,
            atlas_texture: None,
            surface_config: None,
        })
    }

    /// Get a reference to the wgpu instance.
    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    /// Get a reference to the device.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Initialize rendering pipelines for a surface.
    pub fn configure_surface(
        &mut self,
        surface: &wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) {
        let caps = surface.get_capabilities(&self.adapter);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: caps.formats[0],
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&self.device, &config);
        self.surface_config = Some(config.clone());

        self.create_atlas_texture();
        self.create_pipeline(&config);
    }

    fn create_atlas_texture(&mut self) {
        let (w, h) = self.atlas.atlas_dimensions();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Atlas"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.atlas_texture = Some(texture);
    }

    fn create_pipeline(&mut self, config: &wgpu::SurfaceConfiguration) {
        if let Some(ref atlas_texture) = self.atlas_texture {
            let view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.text_pipeline = Some(TextPipeline::new(
                &self.device,
                config,
                atlas_texture,
                &view,
            ));
        }
    }

    /// Upload atlas texture data to GPU.
    fn upload_atlas(&self) {
        if let Some(ref atlas_texture) = self.atlas_texture {
            let (w, h) = self.atlas.atlas_dimensions();
            self.queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: atlas_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                self.atlas.atlas_data(),
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
    }

    /// Build vertices for a single pane's terminal snapshot within a viewport.
    ///
    /// If `viewport` is `None`, renders to the full surface (legacy single-pane mode).
    /// Returns `(vertices, indices)`.
    pub fn build_vertices(
        &mut self,
        snapshot: &ScreenSnapshot,
        color_scheme: &ColorScheme,
        selection: Option<&dyn Fn(usize, usize) -> bool>,
        viewport: Option<ViewportRect>,
    ) -> (Vec<Vertex>, Vec<u32>) {
        let config = match &self.surface_config {
            Some(c) => c,
            None => return (Vec::new(), Vec::new()),
        };

        let (cell_w, cell_h) = self.atlas.cell_size();
        let ascent = self.atlas.ascent();
        let bg = ColorScheme::parse_color(&color_scheme.background);
        let fg = ColorScheme::parse_color(&color_scheme.foreground);
        let sel_bg = ColorScheme::parse_color(&color_scheme.selection_background);
        let sel_fg = ColorScheme::parse_color(&color_scheme.selection_foreground);

        let screen_w = config.width as f32;
        let screen_h = config.height as f32;

        // Offset for sub-region rendering
        let (offset_x, offset_y) = match viewport {
            Some(vp) => (vp.x, vp.y),
            None => (0.0, 0.0),
        };

        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for (row_idx, row) in snapshot.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let x = offset_x + col_idx as f32 * cell_w;
                let y = offset_y + row_idx as f32 * cell_h;

                // Convert to clip space (always relative to full surface)
                let x0 = (x / screen_w) * 2.0 - 1.0;
                let y0 = 1.0 - (y / screen_h) * 2.0;
                let x1 = ((x + cell_w) / screen_w) * 2.0 - 1.0;
                let y1 = 1.0 - ((y + cell_h) / screen_h) * 2.0;

                // Background quad
                let is_selected = selection.map_or(false, |s| s(row_idx, col_idx));
                let bg_color = if is_selected {
                    if cell.bg == pureterm_terminal::Color::Named(pureterm_terminal::NamedColor::Background) {
                        [sel_bg[0] as f32 / 255.0, sel_bg[1] as f32 / 255.0, sel_bg[2] as f32 / 255.0, 1.0]
                    } else {
                        let c = match cell.bg {
                            pureterm_terminal::Color::Rgb(r, g, b) => [r, g, b],
                            pureterm_terminal::Color::Palette(idx) => color_scheme.color_rgb(idx as usize),
                            _ => bg,
                        };
                        let blend = 0.6;
                        [
                            (sel_bg[0] as f32 * (1.0 - blend) + c[0] as f32 * blend) / 255.0,
                            (sel_bg[1] as f32 * (1.0 - blend) + c[1] as f32 * blend) / 255.0,
                            (sel_bg[2] as f32 * (1.0 - blend) + c[2] as f32 * blend) / 255.0,
                            1.0,
                        ]
                    }
                } else {
                    match cell.bg {
                        pureterm_terminal::Color::Rgb(r, g, b) => {
                            [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
                        }
                        pureterm_terminal::Color::Palette(idx) => {
                            let c = color_scheme.color_rgb(idx as usize);
                            [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
                        }
                        _ => [bg[0] as f32 / 255.0, bg[1] as f32 / 255.0, bg[2] as f32 / 255.0, 1.0],
                    }
                };

                let base = vertices.len() as u32;
                vertices.extend_from_slice(&[
                    Vertex { position: [x0, y0], tex_coords: [0.0, 0.0], color: bg_color },
                    Vertex { position: [x1, y0], tex_coords: [0.0, 0.0], color: bg_color },
                    Vertex { position: [x0, y1], tex_coords: [0.0, 0.0], color: bg_color },
                    Vertex { position: [x1, y1], tex_coords: [0.0, 0.0], color: bg_color },
                ]);
                indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);

                // Glyph quad
                if cell.c != ' ' {
                    let (atlas_w, atlas_h) = self.atlas.atlas_dimensions();
                    if let Some(glyph) = self.atlas.get_glyph(0, cell.c) {
                        let tx0 = glyph.x as f32 / atlas_w as f32;
                        let ty0 = glyph.y as f32 / atlas_h as f32;
                        let tx1 = (glyph.x + glyph.width) as f32 / atlas_w as f32;
                        let ty1 = (glyph.y + glyph.height) as f32 / atlas_h as f32;

                        let fg_color = if is_selected {
                            [sel_fg[0] as f32 / 255.0, sel_fg[1] as f32 / 255.0, sel_fg[2] as f32 / 255.0, 1.0]
                        } else {
                            match cell.fg {
                                pureterm_terminal::Color::Rgb(r, g, b) => {
                                    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
                                }
                                pureterm_terminal::Color::Palette(idx) => {
                                    let c = color_scheme.color_rgb(idx as usize);
                                    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
                                }
                                _ => [fg[0] as f32 / 255.0, fg[1] as f32 / 255.0, fg[2] as f32 / 255.0, 1.0],
                            }
                        };

                        let gx0 = x + glyph.x_offset;
                        let gx1 = gx0 + glyph.width as f32;
                        let gy_bottom = y + ascent - glyph.y_offset;
                        let gy_top = gy_bottom - glyph.height as f32;

                        let cgx0 = (gx0 / screen_w) * 2.0 - 1.0;
                        let cgy0 = 1.0 - (gy_top / screen_h) * 2.0;
                        let cgx1 = (gx1 / screen_w) * 2.0 - 1.0;
                        let cgy1 = 1.0 - (gy_bottom / screen_h) * 2.0;

                        let base = vertices.len() as u32;
                        vertices.extend_from_slice(&[
                            Vertex { position: [cgx0, cgy0], tex_coords: [tx0, ty0], color: fg_color },
                            Vertex { position: [cgx1, cgy0], tex_coords: [tx1, ty0], color: fg_color },
                            Vertex { position: [cgx0, cgy1], tex_coords: [tx0, ty1], color: fg_color },
                            Vertex { position: [cgx1, cgy1], tex_coords: [tx1, ty1], color: fg_color },
                        ]);
                        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
                    }
                }
            }
        }

        // Render cursor
        if snapshot.cursor_visible {
            let cx = offset_x + snapshot.cursor_col as f32 * cell_w;
            let cy = offset_y + snapshot.cursor_row as f32 * cell_h;
            let cursor_color = ColorScheme::parse_color(&color_scheme.cursor);

            let cursor_rgba = [
                cursor_color[0] as f32 / 255.0,
                cursor_color[1] as f32 / 255.0,
                cursor_color[2] as f32 / 255.0,
                0.8,
            ];

            let (cx0, cy0, cx1, cy1) = match snapshot.cursor_style {
                pureterm_terminal::CursorStyle::Block => {
                    (
                        (cx / screen_w) * 2.0 - 1.0,
                        1.0 - (cy / screen_h) * 2.0,
                        ((cx + cell_w) / screen_w) * 2.0 - 1.0,
                        1.0 - ((cy + cell_h) / screen_h) * 2.0,
                    )
                }
                pureterm_terminal::CursorStyle::Underline => {
                    let underline_h = 2.0;
                    (
                        (cx / screen_w) * 2.0 - 1.0,
                        1.0 - ((cy + cell_h - underline_h) / screen_h) * 2.0,
                        ((cx + cell_w) / screen_w) * 2.0 - 1.0,
                        1.0 - ((cy + cell_h) / screen_h) * 2.0,
                    )
                }
                pureterm_terminal::CursorStyle::Beam => {
                    let beam_w = 2.0;
                    (
                        (cx / screen_w) * 2.0 - 1.0,
                        1.0 - (cy / screen_h) * 2.0,
                        ((cx + beam_w) / screen_w) * 2.0 - 1.0,
                        1.0 - ((cy + cell_h) / screen_h) * 2.0,
                    )
                }
            };

            let base = vertices.len() as u32;
            vertices.extend_from_slice(&[
                Vertex { position: [cx0, cy0], tex_coords: [0.0, 0.0], color: cursor_rgba },
                Vertex { position: [cx1, cy0], tex_coords: [0.0, 0.0], color: cursor_rgba },
                Vertex { position: [cx0, cy1], tex_coords: [0.0, 0.0], color: cursor_rgba },
                Vertex { position: [cx1, cy1], tex_coords: [0.0, 0.0], color: cursor_rgba },
            ]);
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
        }

        (vertices, indices)
    }

    /// Build vertices for a filled rectangle (used for dividers, borders, menu background).
    pub fn build_rect_vertices(
        &mut self,
        rect: ViewportRect,
        color: [f32; 4],
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
    ) {
        let config = match &self.surface_config {
            Some(c) => c,
            None => return,
        };
        let screen_w = config.width as f32;
        let screen_h = config.height as f32;

        let x0 = (rect.x / screen_w) * 2.0 - 1.0;
        let y0 = 1.0 - (rect.y / screen_h) * 2.0;
        let x1 = ((rect.x + rect.w) / screen_w) * 2.0 - 1.0;
        let y1 = 1.0 - ((rect.y + rect.h) / screen_h) * 2.0;

        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            Vertex { position: [x0, y0], tex_coords: [0.0, 0.0], color },
            Vertex { position: [x1, y0], tex_coords: [0.0, 0.0], color },
            Vertex { position: [x0, y1], tex_coords: [0.0, 0.0], color },
            Vertex { position: [x1, y1], tex_coords: [0.0, 0.0], color },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }

    /// Build vertices for a focus border (4 thin rectangles around the outside of a rect).
    pub fn build_focus_border(
        &mut self,
        rect: ViewportRect,
        color: [f32; 4],
        thickness: f32,
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
    ) {
        // Top border
        self.build_rect_vertices(
            ViewportRect { x: rect.x, y: rect.y, w: rect.w, h: thickness },
            color, vertices, indices,
        );
        // Bottom border
        self.build_rect_vertices(
            ViewportRect { x: rect.x, y: rect.y + rect.h - thickness, w: rect.w, h: thickness },
            color, vertices, indices,
        );
        // Left border
        self.build_rect_vertices(
            ViewportRect { x: rect.x, y: rect.y, w: thickness, h: rect.h },
            color, vertices, indices,
        );
        // Right border
        self.build_rect_vertices(
            ViewportRect { x: rect.x + rect.w - thickness, y: rect.y, w: thickness, h: rect.h },
            color, vertices, indices,
        );
    }

    /// Build vertices for context menu text using the glyph atlas.
    pub fn build_text_vertices(
        &mut self,
        text: &str,
        start_x: f32,
        start_y: f32,
        color: [f32; 4],
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
    ) {
        let config = match &self.surface_config {
            Some(c) => c,
            None => return,
        };
        let screen_w = config.width as f32;
        let screen_h = config.height as f32;
        let ascent = self.atlas.ascent();

        let mut cursor_x = start_x;
        for ch in text.chars() {
            if ch == ' ' {
                let (_, cell_w) = self.atlas.cell_size();
                cursor_x += cell_w * 0.5;
                continue;
            }
            let (atlas_w, atlas_h) = self.atlas.atlas_dimensions();
            if let Some(glyph) = self.atlas.get_glyph(0, ch) {
                let tx0 = glyph.x as f32 / atlas_w as f32;
                let ty0 = glyph.y as f32 / atlas_h as f32;
                let tx1 = (glyph.x + glyph.width) as f32 / atlas_w as f32;
                let ty1 = (glyph.y + glyph.height) as f32 / atlas_h as f32;

                let gx0 = cursor_x + glyph.x_offset;
                let gx1 = gx0 + glyph.width as f32;
                let gy_bottom = start_y + ascent - glyph.y_offset;
                let gy_top = gy_bottom - glyph.height as f32;

                let cgx0 = (gx0 / screen_w) * 2.0 - 1.0;
                let cgy0 = 1.0 - (gy_top / screen_h) * 2.0;
                let cgx1 = (gx1 / screen_w) * 2.0 - 1.0;
                let cgy1 = 1.0 - (gy_bottom / screen_h) * 2.0;

                let base = vertices.len() as u32;
                vertices.extend_from_slice(&[
                    Vertex { position: [cgx0, cgy0], tex_coords: [tx0, ty0], color },
                    Vertex { position: [cgx1, cgy0], tex_coords: [tx1, ty0], color },
                    Vertex { position: [cgx0, cgy1], tex_coords: [tx0, ty1], color },
                    Vertex { position: [cgx1, cgy1], tex_coords: [tx1, ty1], color },
                ]);
                indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);

                cursor_x += glyph.advance;
            }
        }
    }

    /// Submit vertices and indices to the GPU and present.
    pub fn draw(
        &mut self,
        surface: &wgpu::Surface<'static>,
        vertices: &[Vertex],
        indices: &[u32],
        bg_color: [u8; 3],
    ) {
        self.upload_atlas();

        if let Some(ref mut pipeline) = self.text_pipeline {
            pipeline.update_buffers(&self.queue, vertices, indices);
        } else {
            return;
        }

        let output = surface.get_current_texture();
        match output {
            Ok(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Terminal Render Encoder"),
                });

                {
                    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Terminal Render Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: bg_color[0] as f64 / 255.0,
                                    g: bg_color[1] as f64 / 255.0,
                                    b: bg_color[2] as f64 / 255.0,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });

                    if let Some(ref pipeline) = self.text_pipeline {
                        pipeline.render(&mut render_pass);
                    }
                }

                self.queue.submit(std::iter::once(encoder.finish()));
                frame.present();
            }
            Err(e) => {
                log::warn!("Failed to get surface texture: {:?}", e);
            }
        }
    }

    /// Render a terminal snapshot with optional selection highlighting.
    /// Legacy single-pane convenience method.
    pub fn render(
        &mut self,
        surface: &wgpu::Surface<'static>,
        snapshot: &ScreenSnapshot,
        color_scheme: &ColorScheme,
    ) {
        self.render_with_selection(surface, snapshot, color_scheme, None);
    }

    /// Render with selection (legacy single-pane).
    pub fn render_with_selection(
        &mut self,
        surface: &wgpu::Surface<'static>,
        snapshot: &ScreenSnapshot,
        color_scheme: &ColorScheme,
        selection: Option<&dyn Fn(usize, usize) -> bool>,
    ) {
        let bg = ColorScheme::parse_color(&color_scheme.background);
        let (vertices, indices) = self.build_vertices(snapshot, color_scheme, selection, None);
        self.draw(surface, &vertices, &indices, bg);
    }

    /// Get the cell size in pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        self.atlas.cell_size()
    }

    /// Calculate terminal dimensions from pixel size.
    pub fn pixel_to_cells(&self, width: u32, height: u32) -> (usize, usize) {
        let (cell_w, cell_h) = self.cell_size();
        let cols = (width as f32 / cell_w).floor() as usize;
        let rows = (height as f32 / cell_h).floor() as usize;
        (cols.max(1), rows.max(1))
    }

    /// Get a reference to the wgpu queue.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Get a reference to the adapter.
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    /// Get mutable access to the glyph atlas.
    pub fn atlas_mut(&mut self) -> &mut GlyphAtlas {
        &mut self.atlas
    }

    /// Get the glyph atlas.
    pub fn atlas(&self) -> &GlyphAtlas {
        &self.atlas
    }

    /// Get the atlas texture (for egui paint callback integration).
    pub fn atlas_texture(&self) -> Option<&wgpu::Texture> {
        self.atlas_texture.as_ref()
    }

    /// Get the text pipeline.
    pub fn text_pipeline(&self) -> Option<&TextPipeline> {
        self.text_pipeline.as_ref()
    }

    /// Get the surface configuration.
    pub fn surface_config(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.surface_config.as_ref()
    }
}
