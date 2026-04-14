//! Context menu (right-click) for PureTerm.
//! Native implementation without egui dependency.

/// Actions available from the context menu.
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

/// A single item in the context menu.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    pub action: MenuAction,
    pub enabled: bool,
    pub separator_before: bool,
}

/// Context menu state for native rendering (no egui).
pub struct ContextMenuState {
    pub visible: bool,
    pub position: (f32, f32),
    pub hovered_index: Option<usize>,
    pub items: Vec<MenuItem>,
    /// Which pane was right-clicked (for context-dependent actions).
    pub target_pane_id: Option<u64>,
    /// Cached menu width in pixels (recalculated with measure_width).
    menu_width: f32,
}

/// Menu layout constants.
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

    /// Show the menu at the given position.
    pub fn show_at(&mut self, x: f32, y: f32, target_pane_id: u64) {
        self.visible = true;
        self.position = (x, y);
        self.hovered_index = None;
        self.target_pane_id = Some(target_pane_id);
    }

    /// Hide the menu.
    pub fn hide(&mut self) {
        self.visible = false;
        self.hovered_index = None;
    }

    /// Recalculate menu width based on character width from the renderer.
    /// Call this before rendering or hit-testing if char_width changes.
    pub fn measure_width(&mut self, char_width: f32) {
        let mut max_w = 0.0f32;
        for item in &self.items {
            // Monospace font: each character is char_width pixels wide
            let w = item.label.len() as f32 * char_width;
            if w > max_w {
                max_w = w;
            }
        }
        self.menu_width = (max_w + MENU_PADDING_H * 2.0 + 8.0).max(MENU_MIN_WIDTH);
    }

    /// Get the current menu width.
    pub fn width(&self) -> f32 {
        self.menu_width
    }

    /// Compute the bounding rectangle of the entire menu.
    pub fn menu_rect(&self) -> (f32, f32, f32, f32) {
        let total_h = self.total_height();
        (self.position.0, self.position.1, self.menu_width, total_h)
    }

    /// Total height of the menu including separators and padding.
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

    /// Get the rectangle for a specific menu item.
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

    /// Test if a point hits a menu item, return its action.
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

    /// Check if the point is within the menu bounds.
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        if !self.visible {
            return false;
        }
        let (mx, my, mw, mh) = self.menu_rect();
        px >= mx && px <= mx + mw && py >= my && py <= my + mh
    }

    /// Update hovered index based on mouse position.
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

    /// Iterate over items with their positions for rendering.
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

/// Legacy ContextMenu type alias for backwards compatibility.
pub type ContextMenu = ContextMenuState;
