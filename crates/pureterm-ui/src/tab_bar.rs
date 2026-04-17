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
/// Tab bar height accommodates macOS titlebar area (~28pt, ~38pt with notch)
/// plus padding for tab text and close button below.
pub const TAB_BAR_HEIGHT: f32 = 44.0;
pub const TAB_PADDING_H: f32 = 12.0;
pub const TAB_CLOSE_BTN_WIDTH: f32 = 20.0;
pub const TAB_NEW_BTN_WIDTH: f32 = 28.0;
pub const TAB_MIN_WIDTH: f32 = 100.0;
pub const TAB_MAX_WIDTH: f32 = 250.0;
pub const TAB_BAR_BORDER_SIZE: f32 = 1.0;

/// Width reserved for macOS traffic light buttons on the left side of the tab bar.
/// 3 buttons × 12px + 2 gaps × 8px + 7px left margin ≈ 56px
#[cfg(target_os = "macos")]
pub const TRAFFIC_LIGHT_WIDTH: f32 = 56.0;

#[cfg(not(target_os = "macos"))]
pub const TRAFFIC_LIGHT_WIDTH: f32 = 0.0;
