//! PureTerm 的上下文菜单(右键点击)。
//! 不依赖 egui 的原生实现。

/// 上下文菜单中可用的动作。
#[derive(Debug, Clone)]
pub enum MenuAction {
    Copy,
    Paste,
    SelectAll,
    Clear,
    Search,
    SplitRight,
    SplitLeft,
    SplitDown,
    SplitUp,
    ClosePane,
    NewTab,
    CloseTab,
    OpenSettings,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    EditTabTitle,
    EditTabColor,
}

/// 上下文菜单中的单个项。
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    pub action: MenuAction,
    pub enabled: bool,
    pub separator_before: bool,
}

/// 用于原生渲染的上下文菜单状态(无 egui)。
pub struct ContextMenuState {
    pub visible: bool,
    pub position: (f32, f32),
    pub hovered_index: Option<usize>,
    pub items: Vec<MenuItem>,
    /// 右键点击的面板(用于上下文相关的动作)。
    pub target_pane_id: Option<u64>,
    /// 缓存的菜单宽度(像素)(通过 measure_width 重新计算)。
    menu_width: f32,
}

/// 菜单布局常量。
pub const MENU_ITEM_HEIGHT: f32 = 26.0;
pub const MENU_PADDING_H: f32 = 12.0;
pub const MENU_PADDING_V: f32 = 6.0;
pub const MENU_SEPARATOR_HEIGHT: f32 = 6.0;
pub const MENU_MIN_WIDTH: f32 = 180.0;

impl Default for ContextMenuState {
    fn default() -> Self {
        Self {
            visible: false,
            position: (0.0, 0.0),
            hovered_index: None,
            items: Self::default_items(),
            target_pane_id: None,
            menu_width: MENU_MIN_WIDTH,
        }
    }
}

impl ContextMenuState {
    pub fn new() -> Self {
        Self::default()
    }

    fn default_items() -> Vec<MenuItem> {
        vec![
            MenuItem { label: "Copy  Ctrl+Shift+C".into(), action: MenuAction::Copy, enabled: true, separator_before: false },
            MenuItem { label: "Paste  Ctrl+Shift+V".into(), action: MenuAction::Paste, enabled: true, separator_before: false },
            MenuItem { label: "Select All".into(), action: MenuAction::SelectAll, enabled: true, separator_before: false },
            MenuItem { label: "Search  Ctrl+F".into(), action: MenuAction::Search, enabled: true, separator_before: true },
            MenuItem { label: "Clear".into(), action: MenuAction::Clear, enabled: true, separator_before: false },
            MenuItem { label: "Split Right  Ctrl+Shift+D".into(), action: MenuAction::SplitRight, enabled: true, separator_before: true },
            MenuItem { label: "Split Left".into(), action: MenuAction::SplitLeft, enabled: true, separator_before: false },
            MenuItem { label: "Split Down  Ctrl+D".into(), action: MenuAction::SplitDown, enabled: true, separator_before: false },
            MenuItem { label: "Split Up".into(), action: MenuAction::SplitUp, enabled: true, separator_before: false },
            MenuItem { label: "Close Pane".into(), action: MenuAction::ClosePane, enabled: true, separator_before: true },
            MenuItem { label: "New Tab  Ctrl+Shift+T".into(), action: MenuAction::NewTab, enabled: true, separator_before: true },
            MenuItem { label: "Close Tab".into(), action: MenuAction::CloseTab, enabled: true, separator_before: false },
            MenuItem { label: "Rename Tab".into(), action: MenuAction::EditTabTitle, enabled: true, separator_before: true },
            MenuItem { label: "Tab Color".into(), action: MenuAction::EditTabColor, enabled: true, separator_before: false },
            MenuItem { label: "Zoom In  Ctrl++".into(), action: MenuAction::ZoomIn, enabled: true, separator_before: true },
            MenuItem { label: "Zoom Out  Ctrl+-".into(), action: MenuAction::ZoomOut, enabled: true, separator_before: false },
            MenuItem { label: "Zoom Reset  Ctrl+0".into(), action: MenuAction::ZoomReset, enabled: true, separator_before: false },
            MenuItem { label: "Settings...".into(), action: MenuAction::OpenSettings, enabled: true, separator_before: true },
        ]
    }

    /// 在给定位置显示菜单。
    pub fn show_at(&mut self, x: f32, y: f32, target_pane_id: u64) {
        self.visible = true;
        self.position = (x, y);
        self.hovered_index = None;
        self.target_pane_id = Some(target_pane_id);
    }

    /// 隐藏菜单。
    pub fn hide(&mut self) {
        self.visible = false;
        self.hovered_index = None;
    }

    /// 根据渲染器的字符宽度重新计算菜单宽度。
    /// 如果 char_width 发生变化,在渲染或命中测试前调用此方法。
    pub fn measure_width(&mut self, char_width: f32) {
        let mut max_w = 0.0f32;
        for item in &self.items {
            // 等宽字体:每个字符宽 char_width 像素
            let w = item.label.len() as f32 * char_width;
            if w > max_w {
                max_w = w;
            }
        }
        self.menu_width = (max_w + MENU_PADDING_H * 2.0 + 8.0).max(MENU_MIN_WIDTH);
    }

    /// 获取当前菜单宽度。
    pub fn width(&self) -> f32 {
        self.menu_width
    }

    /// 计算整个菜单的边界矩形。
    pub fn menu_rect(&self) -> (f32, f32, f32, f32) {
        let total_h = self.total_height();
        (self.position.0, self.position.1, self.menu_width, total_h)
    }

    /// 包括分隔符和填充的菜单总高度。
    pub fn total_height(&self) -> f32 {
        let mut h = MENU_PADDING_V * 2.0;
        for item in &self.items {
            if item.separator_before {
                h += MENU_SEPARATOR_HEIGHT;
            }
            h += MENU_ITEM_HEIGHT;
        }
        h
    }

    /// 获取特定菜单项的矩形。
    pub fn item_rect(&self, index: usize) -> Option<(f32, f32, f32, f32)> {
        if index >= self.items.len() {
            return None;
        }

        let mut y = self.position.1 + MENU_PADDING_V;
        for (i, item) in self.items.iter().enumerate() {
            if item.separator_before {
                y += MENU_SEPARATOR_HEIGHT;
            }
            if i == index {
                return Some((
                    self.position.0 + MENU_PADDING_H,
                    y,
                    self.menu_width - MENU_PADDING_H * 2.0,
                    MENU_ITEM_HEIGHT,
                ));
            }
            y += MENU_ITEM_HEIGHT;
        }
        None
    }

    /// 测试点是否命中菜单项,返回其动作。
    pub fn hit_test(&self, px: f32, py: f32) -> Option<MenuAction> {
        if !self.visible {
            return None;
        }
        let (mx, my, mw, mh) = self.menu_rect();
        if px < mx || px > mx + mw || py < my || py > my + mh {
            return None;
        }
        for (i, item) in self.items.iter().enumerate() {
            if let Some((ix, iy, iw, ih)) = self.item_rect(i) {
                if px >= ix && px <= ix + iw && py >= iy && py <= iy + ih {
                    if item.enabled {
                        return Some(item.action.clone());
                    }
                    return None;
                }
            }
        }
        None
    }

    /// 检查点是否在菜单边界内。
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        if !self.visible {
            return false;
        }
        let (mx, my, mw, mh) = self.menu_rect();
        px >= mx && px <= mx + mw && py >= my && py <= my + mh
    }

    /// 根据鼠标位置更新悬停索引。
    pub fn update_hover(&mut self, px: f32, py: f32) {
        if !self.visible {
            self.hovered_index = None;
            return;
        }
        for (i, item) in self.items.iter().enumerate() {
            if let Some((ix, iy, iw, ih)) = self.item_rect(i) {
                if px >= ix && px <= ix + iw && py >= iy && py <= iy + ih && item.enabled {
                    self.hovered_index = Some(i);
                    return;
                }
            }
        }
        self.hovered_index = None;
    }

    /// 迭代带有渲染位置的项目。
    pub fn items_with_positions(&self) -> Vec<(usize, &MenuItem, f32, f32)> {
        let mut result = Vec::new();
        let mut y = self.position.1 + MENU_PADDING_V;
        for (i, item) in self.items.iter().enumerate() {
            if item.separator_before {
                y += MENU_SEPARATOR_HEIGHT;
            }
            result.push((i, item, self.position.0 + MENU_PADDING_H, y));
            y += MENU_ITEM_HEIGHT;
        }
        result
    }
}

/// 兼容性的旧版 ContextMenu 类型别名。
pub type ContextMenu = ContextMenuState;
