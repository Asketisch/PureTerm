//! Surface - 单个终端实例
//!
//! 基于 Ghostty 的 Surface.zig：核心终端 surface，包装了
//! 带渲染器、输入处理和 UI 状态的终端实例。

use std::time::Instant;

use pureterm_terminal::input::mods::Mods;
use pureterm_terminal::input::mouse::{MouseEvent, MouseEventType, MouseButton, MouseEncoding};
use pureterm_terminal::input::key::{KeyEvent, Key};
use pureterm_terminal::term::Terminal;

use crate::app::AppMessage;

/// Surface 的唯一标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub u64);

/// Surface - 单个终端实例
pub struct Surface {
    /// 唯一标识符
    pub id: SurfaceId,
    /// 终端状态
    pub terminal: Terminal,
    /// 当前大小（单元格）
    pub size: SurfaceSize,
    /// 鼠标状态
    pub mouse: SurfaceMouse,
    /// 键盘状态
    pub keyboard: KeyboardState,
    /// 此 surface 的字体大小
    pub font_size: f32,
    /// 此 surface 是否被聚焦
    pub focused: bool,
    /// 此 surface 是否为只读
    pub readonly: bool,
    /// 子进程是否已退出
    pub child_exited: bool,
    /// 选择滚动是否处于活动状态
    pub selection_scroll_active: bool,
    /// 鼠标编码模式
    pub mouse_encoding: MouseEncoding,
    /// 上次渲染时间
    pub last_render: Instant,
    /// 待分发的动作
    pub pending_actions: Vec<Action>,
}

/// Surface 大小（单元格和像素）
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceSize {
    /// 宽度（单元格）
    pub cols: usize,
    /// 高度（单元格）
    pub rows: usize,
    /// 宽度（像素）
    pub pixel_width: u32,
    /// 高度（像素）
    pub pixel_height: u32,
    /// 单元格宽度（像素）
    pub cell_width: u32,
    /// 单元格高度（像素）
    pub cell_height: u32,
}

impl SurfaceSize {
    pub fn new(pixel_width: u32, pixel_height: u32, cell_width: u32, cell_height: u32) -> Self {
        let cols = if cell_width > 0 { pixel_width / cell_width } else { 80 } as usize;
        let rows = if cell_height > 0 { pixel_height / cell_height } else { 24 } as usize;
        Self {
            cols,
            rows,
            pixel_width,
            pixel_height,
            cell_width,
            cell_height,
        }
    }
}

impl Surface {
    /// 使用给定终端创建新 surface
    pub fn new(id: SurfaceId, terminal: Terminal, size: SurfaceSize) -> Self {
        Self {
            id,
            terminal,
            size,
            mouse: SurfaceMouse::default(),
            keyboard: KeyboardState::default(),
            font_size: 13.0,
            focused: false,
            readonly: false,
            child_exited: false,
            selection_scroll_active: false,
            mouse_encoding: MouseEncoding::Sgr,
            last_render: Instant::now(),
            pending_actions: Vec::new(),
        }
    }

    /// 处理按键事件
    pub fn key_callback(&mut self, event: KeyEvent) {
        // 如果是只读，不转发输入
        if self.readonly {
            return;
        }

        // 记录按下的键以进行跟踪
        self.keyboard.pressed_key = Some(event.key);
        self.keyboard.last_binding_trigger = Instant::now();

        // 通过终端输入系统处理
        // 终端将编码并写入键
    }

    /// 处理鼠标按钮事件
    pub fn mouse_button_callback(
        &mut self,
        button: MouseButton,
        event_type: MouseEventType,
        row: usize,
        col: usize,
        mods: Mods,
    ) {
        let _event = MouseEvent {
            event: event_type,
            button,
            row,
            col,
            mods,
        };

        // 更新鼠标状态
        self.mouse.last_row = row;
        self.mouse.last_col = col;
        self.mouse.last_button = Some(button);

        // 根据点击处理选择
        match event_type {
            MouseEventType::Press => {
                match button {
                    MouseButton::Left => {
                        if mods.shift() {
                            // 扩展选择
                        } else {
                            // 开始新选择或清除
                        }
                    }
                    MouseButton::Middle => {
                        // 粘贴选择
                    }
                    MouseButton::Right => {
                        // 上下文菜单或扩展选择
                    }
                    _ => {}
                }
            }
            MouseEventType::Release => {
                self.mouse.dragging = false;
            }
            _ => {}
        }
    }

    /// 处理鼠标移动
    pub fn cursor_pos_callback(&mut self, row: usize, col: usize, mods: Mods) {
        let prev_row = self.mouse.last_row;
        let prev_col = self.mouse.last_col;

        self.mouse.last_row = row;
        self.mouse.last_col = col;

        // 根据位置更新光标形状
        self.mouse.update_cursor_shape(mods);

        // 如果正在拖拽，扩展选择
        if self.mouse.dragging {
            // Extend selection to new position
            let _ = (prev_row, prev_col); // 跟踪选择
        }
    }

    /// 处理滚动事件
    pub fn scroll_callback(&mut self, delta_rows: f64, mods: Mods) {
        // 使用修饰符处理滚动
        if !mods.is_empty() {
            // Shift+滚动 = 水平滚动等
        }

        // 滚动终端
        let scroll_amount = (delta_rows.abs() as usize).max(1) * 3;
        if delta_rows > 0.0 {
            self.terminal.scroll_up(scroll_amount);
        } else {
            self.terminal.scroll_down(scroll_amount);
        }
    }

    /// 处理焦点更改
    pub fn focus_callback(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// 处理大小更改
    pub fn size_callback(&mut self, new_size: SurfaceSize) {
        self.size = new_size;
        self.terminal.resize(new_size.cols, new_size.rows);
    }

    /// 处理 DPI/内容比例更改
    pub fn content_scale_callback(&mut self, scale: f32) {
        // 为新 DPI 调整渲染
        let _ = scale;
    }

    /// 获取当前鼠标光标形状
    pub fn cursor_shape(&self) -> CursorShape {
        self.mouse.cursor_shape
    }

    /// 获取所有待处理的动作
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.pending_actions)
    }

    /// 从此 surface 分发动作
    pub fn perform_action(&mut self, action: Action) {
        match action.target() {
            ActionTarget::Surface => {
                // 直接处理 surface 级动作
                self.handle_surface_action(action);
            }
            ActionTarget::App => {
                // 排队让应用处理
                self.pending_actions.push(action);
            }
        }
    }

    fn handle_surface_action(&mut self, action: Action) {
        match action {
            Action::FontSizeDelta(delta) => {
                self.font_size = (self.font_size + delta).clamp(1.0, 256.0);
            }
            Action::FontSizeReset => {
                self.font_size = 13.0;
            }
            Action::CopySelection => {
                // 复制选择到剪贴板
            }
            Action::PasteClipboard => {
                // 从剪贴板粘贴
            }
            Action::ScrollToBottom => {
                self.terminal.scroll_to_bottom();
            }
            Action::ScrollToTop => {
                // 滚动到回滚顶部
            }
            Action::ToggleFullscreen => {
                self.pending_actions.push(Action::ToggleFullscreen);
            }
            _ => {}
        }
    }
}

// ── 鼠标状态 ────────────────────────────────────────────────────────────────

/// 鼠标光标形状
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Text,     // I 形
    Default,  // 箭头
    Pointer,  // 手形
    Crosshair,
    Hidden,
}

/// surface 的鼠标状态
#[derive(Debug)]
pub struct SurfaceMouse {
    /// 上次已知的行位置
    pub last_row: usize,
    /// 上次已知的列位置
    pub last_col: usize,
    /// 当前按下的按钮
    pub last_button: Option<MouseButton>,
    /// 我们是否处于拖拽操作中
    pub dragging: bool,
    /// 当前光标形状
    pub cursor_shape: CursorShape,
    /// 运动去重状态
    pub motion_dedup: pureterm_terminal::input::mouse::MouseMotionDedup,
}

impl Default for SurfaceMouse {
    fn default() -> Self {
        Self {
            last_row: 0,
            last_col: 0,
            last_button: None,
            dragging: false,
            cursor_shape: CursorShape::Text,
            motion_dedup: pureterm_terminal::input::mouse::MouseMotionDedup::new(),
        }
    }
}

impl SurfaceMouse {
    /// 根据修饰符状态更新光标形状（状态机）
    pub fn update_cursor_shape(&mut self, mods: Mods) {
        // 光标形状状态机（来自 Ghostty surface_mouse.zig）：
        // 默认：text（I 形）
        // + Ctrl: default（箭头）
        // + Super: default
        // 在链接上：pointer
        // 矩形选择期间：crosshair
        // 鼠标模式：default（不改变）
        // 隐藏时：不显示
        if self.cursor_shape == CursorShape::Hidden {
            return;
        }

        if mods.ctrl() || mods.super_() {
            self.cursor_shape = CursorShape::Default;
        } else if self.dragging {
            self.cursor_shape = CursorShape::Crosshair;
        } else {
            // 检查是否在链接上（需要终端状态）
            self.cursor_shape = CursorShape::Text;
        }
    }
}

// ── 键盘状态 ─────────────────────────────────────────────────────────────

/// surface 的键盘状态
#[derive(Debug)]
pub struct KeyboardState {
    /// 当前按下的键
    pub pressed_key: Option<Key>,
    /// 上次绑定触发的时间
    pub last_binding_trigger: Instant,
    /// IME 预输入是否处于活动状态
    pub preedit_active: bool,
    /// 预输入文本
    pub preedit_text: String,
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self {
            pressed_key: None,
            last_binding_trigger: Instant::now(),
            preedit_active: false,
            preedit_text: String::new(),
        }
    }
}

// ── 动作系统 ──────────────────────────────────────────────────────────────

/// 可以在应用中分发的动作
#[derive(Debug, Clone)]
pub enum Action {
    // 窗口管理
    Quit,
    NewWindow,
    NewTab,
    CloseTab,
    NewSplit(SplitDirection),
    ToggleFullscreen,
    ToggleMaximize,
    ToggleQuickTerminal,

    // 标签页管理
    MoveTab(isize),
    GotoTab(usize),

    // 分割管理
    ResizeSplit(SplitResizeParameter),
    EqualizeSplits,
    MoveFocus(SplitDirection),

    // 字体
    FontSizeDelta(f32),
    FontSizeReset,

    // 剪贴板
    CopySelection,
    PasteClipboard,

    // 滚动
    ScrollToBottom,
    ScrollToTop,
    ScrollPageUp,
    ScrollPageDown,

    // 视图
    ToggleInspector,
    ToggleSearch,

    // 终端
    ResetTerminal,
    ClearScrollback,

    // 配置
    ReloadConfig,
}

/// 动作的目标
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionTarget {
    /// 动作目标是应用（窗口管理、标签页等）
    App,
    /// 动作目标是当前 surface（字体、剪贴板等）
    Surface,
}

impl Action {
    pub fn target(&self) -> ActionTarget {
        match self {
            Action::Quit
            | Action::NewWindow
            | Action::NewTab
            | Action::CloseTab
            | Action::NewSplit(_)
            | Action::ToggleFullscreen
            | Action::ToggleMaximize
            | Action::ToggleQuickTerminal
            | Action::MoveTab(_)
            | Action::GotoTab(_)
            | Action::ResizeSplit(_)
            | Action::EqualizeSplits
            | Action::MoveFocus(_)
            | Action::ReloadConfig
            | Action::ToggleInspector
            | Action::ToggleSearch => ActionTarget::App,

            Action::FontSizeDelta(_)
            | Action::FontSizeReset
            | Action::CopySelection
            | Action::PasteClipboard
            | Action::ScrollToBottom
            | Action::ScrollToTop
            | Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ResetTerminal
            | Action::ClearScrollback => ActionTarget::Surface,
        }
    }
}

/// 分割创建和焦点移动的方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Left,
    Right,
    Top,
    Bottom,
}

/// 分割调整大小的参数
#[derive(Debug, Clone, Copy)]
pub struct SplitResizeParameter {
    pub direction: SplitDirection,
    pub amount: usize,
}

// ── 应用级动作处理器 ───────────────────────────────────────────────────

/// 处理应用级动作
pub fn handle_app_action(
    action: Action,
    app_mailbox: &mut Vec<AppMessage>,
) {
    match action {
        Action::Quit => {
            app_mailbox.push(AppMessage::Quit);
        }
        Action::NewWindow => {
            app_mailbox.push(AppMessage::NewWindow);
        }
        Action::NewTab => {
            app_mailbox.push(AppMessage::NewTab);
        }
        Action::CloseTab => {
            app_mailbox.push(AppMessage::CloseTab);
        }
        Action::NewSplit(dir) => {
            app_mailbox.push(AppMessage::NewSplit(dir));
        }
        Action::ToggleFullscreen => {
            app_mailbox.push(AppMessage::ToggleFullscreen);
        }
        Action::ToggleMaximize => {
            app_mailbox.push(AppMessage::ToggleMaximize);
        }
        Action::ToggleQuickTerminal => {
            app_mailbox.push(AppMessage::ToggleQuickTerminal);
        }
        Action::MoveTab(delta) => {
            app_mailbox.push(AppMessage::MoveTab(delta));
        }
        Action::GotoTab(idx) => {
            app_mailbox.push(AppMessage::GotoTab(idx));
        }
        Action::ResizeSplit(param) => {
            app_mailbox.push(AppMessage::ResizeSplit(param));
        }
        Action::EqualizeSplits => {
            app_mailbox.push(AppMessage::EqualizeSplits);
        }
        Action::MoveFocus(dir) => {
            app_mailbox.push(AppMessage::MoveFocus(dir));
        }
        Action::ReloadConfig => {
            app_mailbox.push(AppMessage::ReloadConfig);
        }
        Action::ToggleInspector => {
            app_mailbox.push(AppMessage::ToggleInspector);
        }
        Action::ToggleSearch => {
            app_mailbox.push(AppMessage::ToggleSearch);
        }
        _ => {
            // surface 级动作不在这里处理
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surface_size_calculation() {
        let size = SurfaceSize::new(1920, 1080, 10, 20);
        assert_eq!(size.cols, 192);
        assert_eq!(size.rows, 54);
    }

    #[test]
    fn test_mouse_cursor_shape_default() {
        let mouse = SurfaceMouse::default();
        assert_eq!(mouse.cursor_shape, CursorShape::Text);
    }

    #[test]
    fn test_mouse_cursor_shape_ctrl() {
        let mut mouse = SurfaceMouse::default();
        let mods = Mods::from_bits(Mods::CTRL);
        mouse.update_cursor_shape(mods);
        assert_eq!(mouse.cursor_shape, CursorShape::Default);
    }

    #[test]
    fn test_mouse_cursor_shape_dragging() {
        let mut mouse = SurfaceMouse::default();
        mouse.dragging = true;
        mouse.update_cursor_shape(Mods::empty());
        assert_eq!(mouse.cursor_shape, CursorShape::Crosshair);
    }

    #[test]
    fn test_mouse_cursor_shape_hidden_stays_hidden() {
        let mut mouse = SurfaceMouse::default();
        mouse.cursor_shape = CursorShape::Hidden;
        mouse.update_cursor_shape(Mods::empty());
        assert_eq!(mouse.cursor_shape, CursorShape::Hidden);
    }

    #[test]
    fn test_action_target() {
        assert_eq!(Action::Quit.target(), ActionTarget::App);
        assert_eq!(Action::FontSizeDelta(1.0).target(), ActionTarget::Surface);
        assert_eq!(Action::CopySelection.target(), ActionTarget::Surface);
        assert_eq!(Action::NewTab.target(), ActionTarget::App);
    }
}
