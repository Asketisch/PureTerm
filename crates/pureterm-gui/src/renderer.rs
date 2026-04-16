use crate::fonts::GlyphAtlas;
use crate::pipeline::{TextPipeline, Vertex};
use crate::transparency::TransparencyConfig;
use pureterm_core::theme::ColorScheme;
use pureterm_terminal::ScreenSnapshot;

/// 用于子区域渲染的视口矩形
#[derive(Debug, Clone, Copy)]
pub struct ViewportRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 使用 wgpu 的主终端渲染器
pub struct TerminalRenderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    atlas: GlyphAtlas,
    text_pipeline: Option<TextPipeline>,
    atlas_texture: Option<wgpu::Texture>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    transparency_config: TransparencyConfig,
}

impl TerminalRenderer {
    /// 创建新的终端渲染器
    /// `custom_font_path` - 可选的自定义字体文件路径
    pub async fn new(custom_font_path: Option<&str>) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: None,
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find GPU adapter: {:?}", e))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("PureTerm GPU Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create GPU device: {:?}", e))?;

        let mut atlas = GlyphAtlas::new(14.0);
        atlas.load_default_font(custom_font_path).map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            atlas,
            text_pipeline: None,
            atlas_texture: None,
            surface_config: None,
            transparency_config: TransparencyConfig::default(),
        })
    }

    /// 获取 wgpu 实例的引用
    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    /// 获取 wgpu 设备的引用
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// 初始化 surface 的渲染管线
    pub fn configure_surface(
        &mut self,
        surface: &wgpu::Surface<'static>,
        width: u32,
        height: u32,
        transparency: &TransparencyConfig,
    ) {
        let caps = surface.get_capabilities(&self.adapter);

        // 根据透明度设置确定 alpha 模式
        let alpha_mode = if transparency.opacity < 1.0 || transparency.transparent_background {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else {
            wgpu::CompositeAlphaMode::Auto
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: caps.formats[0],
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        self.transparency_config = transparency.clone();
        surface.configure(&self.device, &config);
        self.surface_config = Some(config.clone());

        // 更新管线的屏幕尺寸
        if let Some(ref mut pipeline) = self.text_pipeline {
            pipeline.set_screen_size(&self.queue, width as f32, height as f32);
        }

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

    /// 更新圆角半径
    pub fn set_corner_radius(&mut self, radius: f32) {
        if let Some(ref mut pipeline) = self.text_pipeline {
            pipeline.set_corner_radius(&self.queue, radius);
        }
    }

    /// 上传字形图集纹理数据到 GPU
    fn upload_atlas(&self) {
        if let Some(ref atlas_texture) = self.atlas_texture {
            let (w, h) = self.atlas.atlas_dimensions();
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: atlas_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                self.atlas.atlas_data(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
    }

    /// 为视口内单个窗格的终端快照构建顶点
    ///
    /// 如果 `viewport` 为 None，则渲染到整个表面（传统单窗格模式）
    /// 返回 `(vertices, indices)`
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
        let (underline_pos, underline_thickness, strikethrough_pos, strikethrough_thickness) =
            self.atlas.decoration_metrics();
        let bg = ColorScheme::parse_color(&color_scheme.background);
        let fg = ColorScheme::parse_color(&color_scheme.foreground);
        let sel_bg = ColorScheme::parse_color(&color_scheme.selection_background);
        let sel_fg = ColorScheme::parse_color(&color_scheme.selection_foreground);

        let screen_w = config.width as f32;
        let screen_h = config.height as f32;

        // 子区域渲染的偏移量
        let (offset_x, offset_y) = match viewport {
            Some(vp) => (vp.x, vp.y),
            None => (0.0, 0.0),
        };

        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for (row_idx, row) in snapshot.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                // Skip spacer tail cells — the Wide cell already covers this area
                if cell.wide == pureterm_terminal::WideMode::SpacerTail {
                    continue;
                }

                let x = offset_x + col_idx as f32 * cell_w;
                let y = offset_y + row_idx as f32 * cell_h;

                // Wide characters occupy 2 cells horizontally
                let effective_w = if cell.wide == pureterm_terminal::WideMode::Wide {
                    cell_w * 2.0
                } else {
                    cell_w
                };

                // 转换为裁剪空间（始终相对于整个表面）
                let x0 = (x / screen_w) * 2.0 - 1.0;
                let y0 = 1.0 - (y / screen_h) * 2.0;
                let x1 = ((x + effective_w) / screen_w) * 2.0 - 1.0;
                let y1 = 1.0 - ((y + cell_h) / screen_h) * 2.0;

                // 背景四边形
                let is_selected = selection.map_or(false, |s| s(row_idx, col_idx));

                // 解析原始颜色（palette 0-7 的 bold→bright 映射）
                let raw_fg = match cell.fg {
                    pureterm_terminal::Color::Rgb(r, g, b) => [r, g, b],
                    pureterm_terminal::Color::Palette(idx) => {
                        if idx < 8 && cell.attrs.bold {
                            color_scheme.color_rgb((idx + 8) as usize)
                        } else {
                            color_scheme.color_rgb(idx as usize)
                        }
                    }
                    _ => fg,
                };
                let raw_bg = match cell.bg {
                    pureterm_terminal::Color::Rgb(r, g, b) => [r, g, b],
                    pureterm_terminal::Color::Palette(idx) => color_scheme.color_rgb(idx as usize),
                    _ => bg,
                };

                // 应用反色：交换前景/背景
                let (resolved_fg, resolved_bg) = if cell.attrs.inverse {
                    (raw_bg, raw_fg)
                } else {
                    (raw_fg, raw_bg)
                };

                let bg_color = if is_selected {
                    if cell.bg == pureterm_terminal::Color::Named(pureterm_terminal::NamedColor::Background) && !cell.attrs.inverse {
                        [sel_bg[0] as f32 / 255.0, sel_bg[1] as f32 / 255.0, sel_bg[2] as f32 / 255.0, 1.0]
                    } else {
                        let blend = 0.6;
                        [
                            (sel_bg[0] as f32 * (1.0 - blend) + resolved_bg[0] as f32 * blend) / 255.0,
                            (sel_bg[1] as f32 * (1.0 - blend) + resolved_bg[1] as f32 * blend) / 255.0,
                            (sel_bg[2] as f32 * (1.0 - blend) + resolved_bg[2] as f32 * blend) / 255.0,
                            1.0,
                        ]
                    }
                } else {
                    [resolved_bg[0] as f32 / 255.0, resolved_bg[1] as f32 / 255.0, resolved_bg[2] as f32 / 255.0, 1.0]
                };

                let base = vertices.len() as u32;
                vertices.extend_from_slice(&[
                    Vertex { position: [x0, y0], tex_coords: [0.0, 0.0], color: bg_color },
                    Vertex { position: [x1, y0], tex_coords: [0.0, 0.0], color: bg_color },
                    Vertex { position: [x0, y1], tex_coords: [0.0, 0.0], color: bg_color },
                    Vertex { position: [x1, y1], tex_coords: [0.0, 0.0], color: bg_color },
                ]);
                indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);

                // 字形四边形（隐藏则跳过）
                if cell.c != ' ' && !cell.attrs.hidden {
                    let (atlas_w, atlas_h) = self.atlas.atlas_dimensions();
                    let face_idx = self.atlas.face_index_for_attrs(cell.attrs.bold, cell.attrs.italic);
                    let glyph_opt = self.atlas.get_glyph(face_idx, cell.c);
                    if let Some(glyph) = glyph_opt {
                        let tx0 = glyph.x as f32 / atlas_w as f32;
                        let ty0 = glyph.y as f32 / atlas_h as f32;
                        let tx1 = (glyph.x + glyph.width as u32) as f32 / atlas_w as f32;
                        let ty1 = (glyph.y + glyph.height as u32) as f32 / atlas_h as f32;

                        let fg_color = if is_selected {
                            [sel_fg[0] as f32 / 255.0, sel_fg[1] as f32 / 255.0, sel_fg[2] as f32 / 255.0, 1.0]
                        } else {
                            [resolved_fg[0] as f32 / 255.0, resolved_fg[1] as f32 / 255.0, resolved_fg[2] as f32 / 255.0, 1.0]
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

                // Decorations: underline, strikethrough, overline
                // Resolve decoration color (use underline_color if set, otherwise fg)
                let deco_color_raw = match cell.attrs.underline_color {
                    pureterm_terminal::Color::Rgb(r, g, b) => [r, g, b],
                    pureterm_terminal::Color::Palette(idx) => color_scheme.color_rgb(idx as usize),
                    _ => resolved_fg,
                };
                let deco_color = if is_selected {
                    [sel_fg[0] as f32 / 255.0, sel_fg[1] as f32 / 255.0, sel_fg[2] as f32 / 255.0, 1.0]
                } else {
                    [deco_color_raw[0] as f32 / 255.0, deco_color_raw[1] as f32 / 255.0, deco_color_raw[2] as f32 / 255.0, 1.0]
                };

                let _line_thickness = 1.5f32;

                // Underline (draw for all styles except None)
                if cell.attrs.underline != pureterm_terminal::UnderlineStyle::None {
                    let underline_y = y + ascent + underline_pos;
                    let ut = underline_thickness;
                    match cell.attrs.underline {
                        pureterm_terminal::UnderlineStyle::Double => {
                            // Double underline: two thin lines
                            for dy in [0.0, ut + 1.0] {
                                let uy0 = 1.0 - ((underline_y + dy) / screen_h) * 2.0;
                                let uy1 = 1.0 - ((underline_y + dy + ut) / screen_h) * 2.0;
                                let base = vertices.len() as u32;
                                vertices.extend_from_slice(&[
                                    Vertex { position: [x0, uy0], tex_coords: [0.0, 0.0], color: deco_color },
                                    Vertex { position: [x1, uy0], tex_coords: [0.0, 0.0], color: deco_color },
                                    Vertex { position: [x0, uy1], tex_coords: [0.0, 0.0], color: deco_color },
                                    Vertex { position: [x1, uy1], tex_coords: [0.0, 0.0], color: deco_color },
                                ]);
                                indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
                            }
                        }
                        pureterm_terminal::UnderlineStyle::Curly => {
                            // Curly underline: thicker wavy line (approximated as a thicker bar)
                            let h = ut * 2.0;
                            let uy0 = 1.0 - ((underline_y) / screen_h) * 2.0;
                            let uy1 = 1.0 - ((underline_y + h) / screen_h) * 2.0;
                            let base = vertices.len() as u32;
                            vertices.extend_from_slice(&[
                                Vertex { position: [x0, uy0], tex_coords: [0.0, 0.0], color: deco_color },
                                Vertex { position: [x1, uy0], tex_coords: [0.0, 0.0], color: deco_color },
                                Vertex { position: [x0, uy1], tex_coords: [0.0, 0.0], color: deco_color },
                                Vertex { position: [x1, uy1], tex_coords: [0.0, 0.0], color: deco_color },
                            ]);
                            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
                        }
                        _ => {
                            // Single / Dotted / Dashed: solid line (dotted/dashed require shader support)
                            let uy0 = 1.0 - (underline_y / screen_h) * 2.0;
                            let uy1 = 1.0 - ((underline_y + ut) / screen_h) * 2.0;
                            let base = vertices.len() as u32;
                            vertices.extend_from_slice(&[
                                Vertex { position: [x0, uy0], tex_coords: [0.0, 0.0], color: deco_color },
                                Vertex { position: [x1, uy0], tex_coords: [0.0, 0.0], color: deco_color },
                                Vertex { position: [x0, uy1], tex_coords: [0.0, 0.0], color: deco_color },
                                Vertex { position: [x1, uy1], tex_coords: [0.0, 0.0], color: deco_color },
                            ]);
                            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
                        }
                    }
                }

                // Strikethrough
                if cell.attrs.strikethrough {
                    let strike_y = y + ascent - strikethrough_pos;
                    let sy0 = 1.0 - ((strike_y - strikethrough_thickness * 0.5) / screen_h) * 2.0;
                    let sy1 = 1.0 - ((strike_y + strikethrough_thickness * 0.5) / screen_h) * 2.0;
                    let base = vertices.len() as u32;
                    vertices.extend_from_slice(&[
                        Vertex { position: [x0, sy0], tex_coords: [0.0, 0.0], color: deco_color },
                        Vertex { position: [x1, sy0], tex_coords: [0.0, 0.0], color: deco_color },
                        Vertex { position: [x0, sy1], tex_coords: [0.0, 0.0], color: deco_color },
                        Vertex { position: [x1, sy1], tex_coords: [0.0, 0.0], color: deco_color },
                    ]);
                    indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
                }

                // Overline
                if cell.attrs.overline {
                    let over_y = y;
                    let ol_t = underline_thickness;
                    let oy0 = 1.0 - (over_y / screen_h) * 2.0;
                    let oy1 = 1.0 - ((over_y + ol_t) / screen_h) * 2.0;
                    let base = vertices.len() as u32;
                    vertices.extend_from_slice(&[
                        Vertex { position: [x0, oy0], tex_coords: [0.0, 0.0], color: deco_color },
                        Vertex { position: [x1, oy0], tex_coords: [0.0, 0.0], color: deco_color },
                        Vertex { position: [x0, oy1], tex_coords: [0.0, 0.0], color: deco_color },
                        Vertex { position: [x1, oy1], tex_coords: [0.0, 0.0], color: deco_color },
                    ]);
                    indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
                }
            }
        }

        // 渲染光标
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
                pureterm_terminal::CursorStyle::BlockHollow => {
                    // Hollow block: draw the outline as 4 thin rectangles
                    let _outline_w = 2.0;
                    (
                        (cx / screen_w) * 2.0 - 1.0,
                        1.0 - (cy / screen_h) * 2.0,
                        ((cx + cell_w) / screen_w) * 2.0 - 1.0,
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

    /// 为填充矩形构建顶点（用于分隔线、边框、菜单背景）
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

    /// 为焦点边框构建顶点（矩形外部的 4 条细矩形）
    pub fn build_focus_border(
        &mut self,
        rect: ViewportRect,
        color: [f32; 4],
        thickness: f32,
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
    ) {
        // 顶部边框
        self.build_rect_vertices(
            ViewportRect { x: rect.x, y: rect.y, w: rect.w, h: thickness },
            color, vertices, indices,
        );
        // 底部边框
        self.build_rect_vertices(
            ViewportRect { x: rect.x, y: rect.y + rect.h - thickness, w: rect.w, h: thickness },
            color, vertices, indices,
        );
        // 左边框
        self.build_rect_vertices(
            ViewportRect { x: rect.x, y: rect.y, w: thickness, h: rect.h },
            color, vertices, indices,
        );
        // 右边框
        self.build_rect_vertices(
            ViewportRect { x: rect.x + rect.w - thickness, y: rect.y, w: thickness, h: rect.h },
            color, vertices, indices,
        );
    }

    /// 使用字形图集为上下文菜单文本构建顶点
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
                let (cell_w, _) = self.atlas.cell_size();
                cursor_x += cell_w * 0.5;
                continue;
            }
            let (atlas_w, atlas_h) = self.atlas.atlas_dimensions();
            if let Some(glyph) = self.atlas.get_glyph(0, ch) {
                let tx0 = glyph.x as f32 / atlas_w as f32;
                let ty0 = glyph.y as f32 / atlas_h as f32;
                let tx1 = (glyph.x + glyph.width as u32) as f32 / atlas_w as f32;
                let ty1 = (glyph.y + glyph.height as u32) as f32 / atlas_h as f32;

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

    /// 测量文本的像素宽度
    pub fn measure_text(&mut self, text: &str) -> f32 {
        let mut width = 0.0f32;
        for ch in text.chars() {
            width += self.measure_text_char(ch);
        }
        width
    }

    /// 测量单个字符的像素宽度
    pub fn measure_text_char(&mut self, ch: char) -> f32 {
        if ch == ' ' {
            let (cell_w, _) = self.atlas.cell_size();
            cell_w * 0.5
        } else if let Some(glyph) = self.atlas.get_glyph(0, ch) {
            glyph.advance
        } else {
            0.0
        }
    }

    /// 提交顶点和索引到 GPU 并呈现
    pub fn draw(
        &mut self,
        surface: &wgpu::Surface<'static>,
        vertices: &[Vertex],
        indices: &[u32],
        bg_color: [u8; 3],
    ) {
        let frame = self.draw_to_surface(surface, vertices, indices, bg_color);
        if let Some(frame) = frame {
            frame.present();
        }
    }

    /// 提交顶点和索引到 GPU 但不呈现
    /// 返回 surface 纹理用于进一步渲染（如 egui 覆盖）
    pub fn draw_to_surface(
        &mut self,
        surface: &wgpu::Surface<'static>,
        vertices: &[Vertex],
        indices: &[u32],
        bg_color: [u8; 3],
    ) -> Option<wgpu::SurfaceTexture> {
        self.upload_atlas();

        if let Some(ref mut pipeline) = self.text_pipeline {
            pipeline.update_buffers(&self.queue, vertices, indices);
        } else {
            return None;
        }

        let frame = surface.get_current_texture();
        let frame = match frame {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Timeout => {
                log::warn!("get_current_texture timed out");
                return None;
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                log::warn!("get_current_texture outdated");
                return None;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("get_current_texture lost");
                return None;
            }
            other => {
                log::warn!("get_current_texture returned: {:?}", other);
                return None;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Terminal Render Encoder"),
        });

        {
            let clear_color = self.transparency_config.clear_color(bg_color);
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Terminal Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let Some(ref pipeline) = self.text_pipeline {
                pipeline.render(&mut render_pass);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        Some(frame)
    }

    /// 渲染终端快照（可选选区高亮）
    /// 传统单窗格便捷方法
    pub fn render(
        &mut self,
        surface: &wgpu::Surface<'static>,
        snapshot: &ScreenSnapshot,
        color_scheme: &ColorScheme,
    ) {
        self.render_with_selection(surface, snapshot, color_scheme, None);
    }

    /// 带选区渲染（传统单窗格）
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

    /// 获取单元格尺寸（像素）
    pub fn cell_size(&self) -> (f32, f32) {
        self.atlas.cell_size()
    }

    /// 从像素尺寸计算终端尺寸
    pub fn pixel_to_cells(&self, width: u32, height: u32) -> (usize, usize) {
        let (cell_w, cell_h) = self.cell_size();
        let cols = (width as f32 / cell_w).floor() as usize;
        let rows = (height as f32 / cell_h).floor() as usize;
        (cols.max(1), rows.max(1))
    }

    /// 获取 wgpu 队列的引用
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// 获取 wgpu 适配器的引用
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    /// 获取字形图集的可变引用
    pub fn atlas_mut(&mut self) -> &mut GlyphAtlas {
        &mut self.atlas
    }

    /// 获取字形图集
    pub fn atlas(&self) -> &GlyphAtlas {
        &self.atlas
    }

    /// 获取图集纹理（用于 egui 绘制回调集成）
    pub fn atlas_texture(&self) -> Option<&wgpu::Texture> {
        self.atlas_texture.as_ref()
    }

    /// 获取文本管线
    pub fn text_pipeline(&self) -> Option<&TextPipeline> {
        self.text_pipeline.as_ref()
    }

    /// 获取表面配置
    pub fn surface_config(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.surface_config.as_ref()
    }
}
