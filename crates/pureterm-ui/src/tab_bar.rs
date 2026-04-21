/// Tab bar rendering state (native, no egui).
#[derive(Debug)]
pub struct TabBarState {
    /// Whether the tab bar is visible.
    pub visible: bool,
    /// Current mouse hover state for tabs.
    pub hovered_tab_index: Option<usize>,
    /// Whether the "+" new tab button is hovered.
    pub hovered_new_tab: bool,
    /// Layout info computed at render time.
    pub tab_layouts: Vec<TabLayout>,
    /// "+" new tab button rect.
    pub new_tab_rect: ViewportRect,
    /// Tab bar height in pixels.
    pub height: f32,
    /// Whether tab bar state has changed and needs re-rendering.
    pub tab_bar_dirty: bool,
    /// Index of the tab whose close button is being hovered.
    pub hovered_close_tab_index: Option<usize>,
    /// DPI scale factor (physical pixels per logical point).
    pub scale: f32,
    /// Scaled tab bar height (physical pixels).
    pub bar_h: f32,
    /// Scaled tab item height (physical pixels).
    pub item_h: f32,
    /// Scaled tab item y offset (physical pixels).
    pub item_y: f32,
    /// Scaled close button width (physical pixels).
    pub close_btn_w: f32,
    /// Scaled new tab button width (physical pixels).
    pub new_btn_w: f32,
    /// Scaled horizontal padding (physical pixels).
    pub padding_h: f32,
}

impl TabBarState {
    pub fn new() -> Self {
        Self {
            visible: true,
            hovered_tab_index: None,
            hovered_new_tab: false,
            tab_layouts: Vec::new(),
            new_tab_rect: ViewportRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
            height: TAB_BAR_HEIGHT,
            tab_bar_dirty: false,
            hovered_close_tab_index: None,
            scale: 1.0,
            bar_h: TAB_BAR_HEIGHT,
            item_h: TAB_ITEM_HEIGHT,
            item_y: TAB_ITEM_Y,
            close_btn_w: TAB_CLOSE_BTN_WIDTH,
            new_btn_w: TAB_NEW_BTN_WIDTH,
            padding_h: TAB_PADDING_H,
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
    /// Optional tab color (hex string like "#e06c75").
    pub color: Option<String>,
    /// The tab rect in screen coordinates.
    pub rect: ViewportRect,
    /// Close button rect (full tab height, for hit testing).
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
/// Tab bar height matches macOS traffic light buttons (28pt),
/// same as Ghostty's tab bar — buttons sit naturally centered.
pub const TAB_BAR_HEIGHT: f32 = 28.0;
/// Tab item fills the entire tab bar height.
pub const TAB_ITEM_HEIGHT: f32 = 28.0;
/// Vertical offset — items start at top of tab bar.
pub const TAB_ITEM_Y: f32 = 0.0;
pub const TAB_PADDING_H: f32 = 12.0;
pub const TAB_CLOSE_BTN_WIDTH: f32 = 28.0;
pub const TAB_NEW_BTN_WIDTH: f32 = 36.0;
pub const TAB_BAR_BORDER_SIZE: f32 = 1.0;

/// Width reserved for macOS traffic light buttons on the left side of the tab bar.
/// Matches Ghostty's window buttons backdrop width, accounting for actual macOS
/// traffic light area including left margin, three buttons, gaps, and right padding.
#[cfg(target_os = "macos")]
pub const TRAFFIC_LIGHT_WIDTH: f32 = 92.0;

#[cfg(not(target_os = "macos"))]
pub const TRAFFIC_LIGHT_WIDTH: f32 = 0.0;
