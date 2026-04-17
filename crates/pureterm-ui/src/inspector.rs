//! 终端检查器 — 用于终端状态调试的调试工具。
//!
//! 提供一个调试面板来查看终端状态、渲染器信息、
//! 键盘事件和 VT 解析细节。

use egui::{CollapsingHeader, ScrollArea, TextEdit, Ui};

/// 用于调试的终端状态信息。
#[derive(Debug, Clone, Default)]
pub struct TerminalDebugInfo {
    /// 当前光标位置。
    pub cursor_x: usize,
    pub cursor_y: usize,
    /// 屏幕尺寸。
    pub cols: usize,
    pub rows: usize,
    /// 当前模式标志 (DECSET/DECRST)。
    pub mode_flags: Vec<String>,
    /// 标题（如果设置）。
    pub title: Option<String>,
    /// 当前目录（如果报告）。
    pub cwd: Option<String>,
}

/// 渲染器调试信息。
#[derive(Debug, Clone, Default)]
pub struct RendererDebugInfo {
    /// 帧计数。
    pub frame_count: u64,
    /// FPS（如果可用）。
    pub fps: Option<f32>,
    /// 纹理内存使用。
    pub texture_memory: Option<u64>,
    /// 上一帧的绘制调用次数。
    pub draw_calls: usize,
}

/// Grid 状态信息。
#[derive(Debug, Clone, Default)]
pub struct GridDebugInfo {
    /// Grid 行数。
    pub rows: usize,
    /// Grid 列数。
    pub cols: usize,
    /// 回滚历史大小。
    pub scrollback_size: usize,
    /// 当前滚动偏移量(视口在历史中滚动了多远)。
    pub scroll_offset: usize,
    /// 光标行位置。
    pub cursor_row: usize,
    /// 光标列位置。
    pub cursor_col: usize,
    /// 可见区域中包裹行的数量。
    pub wrapped_lines: usize,
}

/// 检查器的终端模式标志。
#[derive(Debug, Clone, Default)]
pub struct ModesDebugInfo {
    /// 鼠标报告模式 1000 (点击)。
    pub mouse_report_click: bool,
    /// 鼠标报告模式 1002 (拖拽)。
    pub mouse_drag: bool,
    /// 鼠标报告模式 1003 (移动)。
    pub mouse_motion: bool,
    /// 括号粘贴模式 (2004)。
    pub bracketed_paste: bool,
    /// 备用屏幕活动。
    pub alternate_screen: bool,
    /// 应用光标键模式。
    pub app_cursor: bool,
    /// 自动换行模式。
    pub auto_wrap: bool,
    /// 原点模式。
    pub origin_mode: bool,
    /// 焦点跟踪模式 (1004)。
    pub focus_tracking: bool,
    /// Kitty 键盘协议标志位掩码。
    pub kitty_keyboard_flags: u8,
}

/// 字体和 atlas 调试信息。
#[derive(Debug, Clone, Default)]
pub struct FontAtlasDebugInfo {
    /// 配置的字体大小(磅)。
    pub font_size: f32,
    /// 单元格宽度(像素)。
    pub cell_width: f32,
    /// 单元格高度(像素)。
    pub cell_height: f32,
    /// 字体上升量(像素)。
    pub ascent: f32,
    /// 已加载字体面的数量。
    pub loaded_faces: usize,
    /// Atlas 宽度(像素)。
    pub atlas_width: u32,
    /// Atlas 高度(像素)。
    pub atlas_height: u32,
    /// Atlas 中缓存的字形数量。
    pub cached_glyphs: usize,
}

/// 性能调试信息。
#[derive(Debug, Clone, Default)]
pub struct PerfDebugInfo {
    /// 启动以来的总帧数。
    pub frame_count: u64,
    /// 当前每秒帧数。
    pub fps: f32,
    /// 上一帧的顶点数量。
    pub vertex_count: usize,
    /// 估计的绘制调用次数。
    pub draw_calls: usize,
}

/// 检查器中显示的配置调试信息。
#[derive(Debug, Clone, Default)]
pub struct ConfigDebugInfo {
    /// 字体族名称。
    pub font_family: String,
    /// 字体大小(磅)。
    pub font_size: f32,
    /// 活动主题名称。
    pub theme: String,
    /// 回滚行限制。
    pub scrollback_limit: u32,
    /// 光标样式名称。
    pub cursor_style: String,
    /// 光标闪烁是否启用。
    pub cursor_blink: bool,
}

/// 键盘事件历史记录。
#[derive(Debug, Clone)]
pub struct KeyEventRecord {
    /// 事件时间戳。
    pub time: String,
    /// 按键名称。
    pub key: String,
    /// 按下的修饰键。
    pub mods: String,
    /// 是否被消费。
    pub consumed: bool,
}

/// 检查器状态。
pub struct InspectorState {
    /// 检查器是否可见。
    visible: bool,
    /// 终端调试信息。
    pub terminal: TerminalDebugInfo,
    /// 渲染器调试信息。
    pub renderer: RendererDebugInfo,
    /// Grid 状态调试信息。
    pub grid: GridDebugInfo,
    /// 终端模式调试信息。
    pub modes: ModesDebugInfo,
    /// 字体和 atlas 调试信息。
    pub font_atlas: FontAtlasDebugInfo,
    /// 性能调试信息。
    pub perf: PerfDebugInfo,
    /// 配置调试信息。
    pub config: ConfigDebugInfo,
    /// 键盘事件历史(循环缓冲区)。
    key_history: Vec<KeyEventRecord>,
    /// 最大键盘历史大小。
    max_key_history: usize,
    /// VT 事件日志。
    vt_log: Vec<String>,
    /// 最大 VT 日志大小。
    max_vt_log: usize,
}

impl Default for InspectorState {
    fn default() -> Self {
        Self {
            visible: false,
            terminal: TerminalDebugInfo::default(),
            renderer: RendererDebugInfo::default(),
            grid: GridDebugInfo::default(),
            modes: ModesDebugInfo::default(),
            font_atlas: FontAtlasDebugInfo::default(),
            perf: PerfDebugInfo::default(),
            config: ConfigDebugInfo::default(),
            key_history: Vec::with_capacity(100),
            max_key_history: 100,
            vt_log: Vec::with_capacity(1000),
            max_vt_log: 1000,
        }
    }
}

impl InspectorState {
    /// 创建新的检查器状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 切换检查器可见性。
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// 检查检查器是否可见。
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 显示/隐藏检查器。
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// 更新终端调试信息。
    pub fn update_terminal(&mut self, info: TerminalDebugInfo) {
        self.terminal = info;
    }

    /// 更新渲染器调试信息。
    pub fn update_renderer(&mut self, info: RendererDebugInfo) {
        self.renderer = info;
    }

    /// 更新 Grid 状态调试信息。
    pub fn update_grid(&mut self, info: GridDebugInfo) {
        self.grid = info;
    }

    /// 更新终端模式调试信息。
    pub fn update_modes(&mut self, info: ModesDebugInfo) {
        self.modes = info;
    }

    /// 更新字体和 atlas 调试信息。
    pub fn update_font_atlas(&mut self, info: FontAtlasDebugInfo) {
        self.font_atlas = info;
    }

    /// 更新性能调试信息。
    pub fn update_perf(&mut self, info: PerfDebugInfo) {
        self.perf = info;
    }

    /// 更新配置调试信息。
    pub fn update_config(&mut self, info: ConfigDebugInfo) {
        self.config = info;
    }

    /// 记录一个键盘事件。
    pub fn record_key(&mut self, key: String, mods: String, consumed: bool) {
        let time = chrono_lite_timestamp();
        self.key_history.push(KeyEventRecord {
            time,
            key,
            mods,
            consumed,
        });
        if self.key_history.len() > self.max_key_history {
            self.key_history.remove(0);
        }
    }

    /// 记录一个 VT 事件。
    pub fn log_vt(&mut self, event: String) {
        self.vt_log.push(event);
        if self.vt_log.len() > self.max_vt_log {
            self.vt_log.remove(0);
        }
    }

    /// 清除键盘历史。
    pub fn clear_key_history(&mut self) {
        self.key_history.clear();
    }

    /// 清除 VT 日志。
    pub fn clear_vt_log(&mut self) {
        self.vt_log.clear();
    }

    /// 绘制检查器 UI。
    pub fn draw(&mut self, ui: &mut Ui) {
        if !self.visible {
            return;
        }

        egui::Window::new("Terminal Inspector")
            .default_width(400.0)
            .default_height(300.0)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                self.draw_content(ui);
            });
    }

    fn draw_content(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().id_salt("inspector_scroll").show(ui, |ui| {
            // 终端部分
            CollapsingHeader::new("Terminal State")
                .default_open(true)
                .show(ui, |ui| {
                    self.draw_terminal_section(ui);
                });

            // Grid State section
            CollapsingHeader::new("Grid State")
                .default_open(false)
                .show(ui, |ui| {
                    self.draw_grid_section(ui);
                });

            // Terminal Modes section
            CollapsingHeader::new("Terminal Modes")
                .default_open(false)
                .show(ui, |ui| {
                    self.draw_modes_section(ui);
                });

            // Font & Atlas section
            CollapsingHeader::new("Font & Atlas")
                .default_open(false)
                .show(ui, |ui| {
                    self.draw_font_atlas_section(ui);
                });

            // Performance section
            CollapsingHeader::new("Performance")
                .default_open(false)
                .show(ui, |ui| {
                    self.draw_perf_section(ui);
                });

            // Configuration section
            CollapsingHeader::new("Configuration")
                .default_open(false)
                .show(ui, |ui| {
                    self.draw_config_section(ui);
                });

            // 渲染器部分
            CollapsingHeader::new("Renderer")
                .default_open(false)
                .show(ui, |ui| {
                    self.draw_renderer_section(ui);
                });

            // 键盘历史部分
            CollapsingHeader::new("Keyboard History")
                .default_open(false)
                .show(ui, |ui| {
                    self.draw_keyboard_section(ui);
                });

            // VT 日志部分
            CollapsingHeader::new("VT Event Log")
                .default_open(false)
                .show(ui, |ui| {
                    self.draw_vt_log_section(ui);
                });
        });
    }

    fn draw_terminal_section(&self, ui: &mut Ui) {
        egui::Grid::new("terminal_grid").num_columns(2).show(ui, |ui| {
            ui.label("Cursor:");
            ui.label(format!("({}, {})", self.terminal.cursor_x, self.terminal.cursor_y));
            ui.end_row();

            ui.label("Size:");
            ui.label(format!("{}x{}", self.terminal.cols, self.terminal.rows));
            ui.end_row();

            if let Some(ref title) = self.terminal.title {
                ui.label("Title:");
                ui.label(title.as_str());
                ui.end_row();
            }

            if let Some(ref cwd) = self.terminal.cwd {
                ui.label("CWD:");
                ui.label(cwd.as_str());
                ui.end_row();
            }

            if !self.terminal.mode_flags.is_empty() {
                ui.label("Modes:");
                ui.horizontal_wrapped(|ui| {
                    for flag in &self.terminal.mode_flags {
                        ui.label(flag.as_str());
                    }
                });
                ui.end_row();
            }
        });
    }

    fn draw_grid_section(&self, ui: &mut Ui) {
        egui::Grid::new("grid_debug_grid").num_columns(2).show(ui, |ui| {
            ui.label("Grid Rows:");
            ui.label(self.grid.rows.to_string());
            ui.end_row();

            ui.label("Grid Cols:");
            ui.label(self.grid.cols.to_string());
            ui.end_row();

            ui.label("Scrollback Size:");
            ui.label(self.grid.scrollback_size.to_string());
            ui.end_row();

            ui.label("Scroll Offset:");
            ui.label(self.grid.scroll_offset.to_string());
            ui.end_row();

            ui.label("Cursor Row:");
            ui.label(self.grid.cursor_row.to_string());
            ui.end_row();

            ui.label("Cursor Col:");
            ui.label(self.grid.cursor_col.to_string());
            ui.end_row();

            ui.label("Wrapped Lines:");
            ui.label(self.grid.wrapped_lines.to_string());
            ui.end_row();
        });
    }

    fn draw_modes_section(&self, ui: &mut Ui) {
        egui::Grid::new("modes_debug_grid").num_columns(2).show(ui, |ui| {
            ui.label("Mouse Click (1000):");
            ui.label(bool_str(self.modes.mouse_report_click));
            ui.end_row();

            ui.label("Mouse Drag (1002):");
            ui.label(bool_str(self.modes.mouse_drag));
            ui.end_row();

            ui.label("Mouse Motion (1003):");
            ui.label(bool_str(self.modes.mouse_motion));
            ui.end_row();

            ui.label("Bracketed Paste:");
            ui.label(bool_str(self.modes.bracketed_paste));
            ui.end_row();

            ui.label("Alternate Screen:");
            ui.label(bool_str(self.modes.alternate_screen));
            ui.end_row();

            ui.label("App Cursor:");
            ui.label(bool_str(self.modes.app_cursor));
            ui.end_row();

            ui.label("Auto Wrap:");
            ui.label(bool_str(self.modes.auto_wrap));
            ui.end_row();

            ui.label("Origin Mode:");
            ui.label(bool_str(self.modes.origin_mode));
            ui.end_row();

            ui.label("Focus Tracking (1004):");
            ui.label(bool_str(self.modes.focus_tracking));
            ui.end_row();

            ui.label("Kitty Keyboard Flags:");
            ui.label(format!("0x{:02X}", self.modes.kitty_keyboard_flags));
            ui.end_row();
        });
    }

    fn draw_font_atlas_section(&self, ui: &mut Ui) {
        egui::Grid::new("font_atlas_grid").num_columns(2).show(ui, |ui| {
            ui.label("Font Size:");
            ui.label(format!("{:.1} pt", self.font_atlas.font_size));
            ui.end_row();

            ui.label("Cell Size:");
            ui.label(format!("{:.1} x {:.1} px", self.font_atlas.cell_width, self.font_atlas.cell_height));
            ui.end_row();

            ui.label("Ascent:");
            ui.label(format!("{:.1} px", self.font_atlas.ascent));
            ui.end_row();

            ui.label("Loaded Faces:");
            ui.label(self.font_atlas.loaded_faces.to_string());
            ui.end_row();

            ui.label("Atlas Dimensions:");
            ui.label(format!("{} x {} px", self.font_atlas.atlas_width, self.font_atlas.atlas_height));
            ui.end_row();

            ui.label("Cached Glyphs:");
            ui.label(self.font_atlas.cached_glyphs.to_string());
            ui.end_row();
        });
    }

    fn draw_perf_section(&self, ui: &mut Ui) {
        egui::Grid::new("perf_debug_grid").num_columns(2).show(ui, |ui| {
            ui.label("Frame Count:");
            ui.label(self.perf.frame_count.to_string());
            ui.end_row();

            ui.label("FPS:");
            ui.label(format!("{:.1}", self.perf.fps));
            ui.end_row();

            ui.label("Vertex Count:");
            ui.label(self.perf.vertex_count.to_string());
            ui.end_row();

            ui.label("Draw Calls:");
            ui.label(self.perf.draw_calls.to_string());
            ui.end_row();
        });
    }

    fn draw_config_section(&self, ui: &mut Ui) {
        egui::Grid::new("config_debug_grid").num_columns(2).show(ui, |ui| {
            ui.label("Font Family:");
            ui.label(self.config.font_family.as_str());
            ui.end_row();

            ui.label("Font Size:");
            ui.label(format!("{:.1}", self.config.font_size));
            ui.end_row();

            ui.label("Theme:");
            ui.label(self.config.theme.as_str());
            ui.end_row();

            ui.label("Scrollback Limit:");
            ui.label(self.config.scrollback_limit.to_string());
            ui.end_row();

            ui.label("Cursor Style:");
            ui.label(self.config.cursor_style.as_str());
            ui.end_row();

            ui.label("Cursor Blink:");
            ui.label(bool_str(self.config.cursor_blink));
            ui.end_row();
        });
    }

    fn draw_renderer_section(&self, ui: &mut Ui) {
        egui::Grid::new("renderer_grid").num_columns(2).show(ui, |ui| {
            ui.label("Frame:");
            ui.label(self.renderer.frame_count.to_string());
            ui.end_row();

            if let Some(fps) = self.renderer.fps {
                ui.label("FPS:");
                ui.label(format!("{:.1}", fps));
                ui.end_row();
            }

            if let Some(mem) = self.renderer.texture_memory {
                ui.label("Texture Memory:");
                ui.label(format_bytes(mem));
                ui.end_row();
            }

            ui.label("Draw Calls:");
            ui.label(self.renderer.draw_calls.to_string());
            ui.end_row();
        });
    }

    fn draw_keyboard_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("Clear").clicked() {
                self.clear_key_history();
            }
        });

        egui::ScrollArea::new([false, true])
            .max_height(200.0)
            .show(ui, |ui| {
                egui::Grid::new("key_grid")
                    .num_columns(4)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Time");
                        ui.label("Key");
                        ui.label("Modifiers");
                        ui.label("Consumed");
                        ui.end_row();

                        for event in self.key_history.iter().rev().take(50) {
                            ui.label(event.time.as_str());
                            ui.label(event.key.as_str());
                            ui.label(event.mods.as_str());
                            ui.label(if event.consumed { "yes" } else { "no" });
                            ui.end_row();
                        }
                    });
            });
    }

    fn draw_vt_log_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("Clear").clicked() {
                self.clear_vt_log();
            }
        });

        egui::ScrollArea::new([false, true])
            .max_height(300.0)
            .show(ui, |ui| {
                let log_text = self.vt_log.join("\n");
                TextEdit::multiline(&mut log_text.as_str())
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .show(ui);
            });
    }
}

/// 将布尔值格式化为彩色字符串标签。
fn bool_str(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

/// 简单的时间戳函数（避免依赖 chrono）。
fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    format!("{}.{:03}", secs, millis)
}

/// 将字节格式化为人类可读的字符串。
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspector_state_creation() {
        let state = InspectorState::new();
        assert!(!state.is_visible());
        assert_eq!(state.terminal.cursor_x, 0);
    }

    #[test]
    fn test_key_recording() {
        let mut state = InspectorState::new();
        state.record_key("a".to_string(), "Ctrl".to_string(), false);
        state.record_key("b".to_string(), "Shift".to_string(), true);
        assert_eq!(state.key_history.len(), 2);
    }

    #[test]
    fn test_vt_log() {
        let mut state = InspectorState::new();
        state.log_vt("ESC [ 1 m".to_string());
        state.log_vt("OSC 133 ; A ST".to_string());
        assert_eq!(state.vt_log.len(), 2);
    }

    #[test]
    fn test_toggle() {
        let mut state = InspectorState::new();
        assert!(!state.is_visible());
        state.toggle();
        assert!(state.is_visible());
        state.toggle();
        assert!(!state.is_visible());
    }

    #[test]
    fn test_grid_debug_info_update() {
        let mut state = InspectorState::new();
        state.update_grid(GridDebugInfo {
            rows: 50,
            cols: 120,
            scrollback_size: 1000,
            scroll_offset: 42,
            cursor_row: 10,
            cursor_col: 20,
            wrapped_lines: 5,
        });
        assert_eq!(state.grid.rows, 50);
        assert_eq!(state.grid.cols, 120);
        assert_eq!(state.grid.scrollback_size, 1000);
        assert_eq!(state.grid.scroll_offset, 42);
        assert_eq!(state.grid.cursor_row, 10);
        assert_eq!(state.grid.cursor_col, 20);
        assert_eq!(state.grid.wrapped_lines, 5);
    }

    #[test]
    fn test_modes_debug_info_update() {
        let mut state = InspectorState::new();
        state.update_modes(ModesDebugInfo {
            mouse_report_click: true,
            bracketed_paste: true,
            kitty_keyboard_flags: 0x03,
            ..Default::default()
        });
        assert!(state.modes.mouse_report_click);
        assert!(state.modes.bracketed_paste);
        assert!(!state.modes.mouse_drag);
        assert_eq!(state.modes.kitty_keyboard_flags, 0x03);
    }

    #[test]
    fn test_font_atlas_debug_info_update() {
        let mut state = InspectorState::new();
        state.update_font_atlas(FontAtlasDebugInfo {
            font_size: 14.0,
            cell_width: 8.4,
            cell_height: 18.0,
            ascent: 14.0,
            loaded_faces: 3,
            atlas_width: 1024,
            atlas_height: 1024,
            cached_glyphs: 512,
        });
        assert_eq!(state.font_atlas.font_size, 14.0);
        assert_eq!(state.font_atlas.loaded_faces, 3);
        assert_eq!(state.font_atlas.cached_glyphs, 512);
    }

    #[test]
    fn test_perf_debug_info_update() {
        let mut state = InspectorState::new();
        state.update_perf(PerfDebugInfo {
            frame_count: 1234,
            fps: 60.0,
            vertex_count: 10000,
            draw_calls: 5,
        });
        assert_eq!(state.perf.frame_count, 1234);
        assert_eq!(state.perf.fps, 60.0);
        assert_eq!(state.perf.vertex_count, 10000);
    }

    #[test]
    fn test_config_debug_info_update() {
        let mut state = InspectorState::new();
        state.update_config(ConfigDebugInfo {
            font_family: "JetBrains Mono".to_string(),
            font_size: 14.0,
            theme: "dark".to_string(),
            scrollback_limit: 25000,
            cursor_style: "block".to_string(),
            cursor_blink: true,
        });
        assert_eq!(state.config.font_family, "JetBrains Mono");
        assert_eq!(state.config.scrollback_limit, 25000);
        assert!(state.config.cursor_blink);
    }
}
