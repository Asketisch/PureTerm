/// Tab bar rendering state (native, no egui).
#[derive(Debug)]
pub struct TabBarState {
    /// Whether the tab bar is visible.
    pub visible: bool,
    /// Current mouse hover state.
    pub hovered_tab_index: Option<usize>,
    /// Layout info computed at render time.
    pub tab_layouts: Vec<TabLayout>,
    /// Tab bar height in pixels.
    pub height: f32,
}

impl TabBarState {
    pub fn new() -> Self {
        Self {
            visible: true,
            hovered_tab_index: None,
            tab_layouts: Vec::new(),
            height: TAB_BAR_HEIGHT,
        }
    }
}

impl Default for TabBarState {
    fn default() -> Self {
        Self::new()
    }
}

/// Layout info for a single tab.
#[derive(Debug, Clone)]
pub struct TabLayout {
    /// The tab ID.
    pub tab_id: u64,
    /// The tab title.
    pub title: String,
    /// The tab rect in screen coordinates.
    pub rect: ViewportRect,
    /// Close button rect, if present.
    pub close_button_rect: Option<ViewportRect>,
}

/// Viewport rectangle in pixels.
#[derive(Debug, Clone, Copy)]
pub struct ViewportRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl ViewportRect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

/// Layout constants.
pub const TAB_BAR_HEIGHT: f32 = 36.0;
pub const TAB_PADDING_H: f32 = 12.0;
pub const TAB_CLOSE_BTN_WIDTH: f32 = 20.0;
pub const TAB_MIN_WIDTH: f32 = 100.0;
pub const TAB_MAX_WIDTH: f32 = 200.0;
pub const TAB_BAR_BORDER_SIZE: f32 = 1.0;
pub const TAB_TITLE_OFFSET_X: f32 = 8.0;
pub const TAB_CLOSE_BTN_OFFSET_X: f32 = 4.0;
