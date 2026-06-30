//! GAP-19: Dual Sidebar — left (Workspace + Git summary) and right
//! (Find / Feed / Git modes). State lives here; views in `left` / `right`.

pub mod left;
pub mod right;

/// Sidebar width bounds (px). Config values are clamped to this range.
pub const MIN_WIDTH: f32 = 120.0;
pub const MAX_WIDTH: f32 = 480.0;
pub const DEFAULT_WIDTH: f32 = 240.0;

/// Which sidebar divider is being dragged (GAP-19 drag-to-resize).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSide {
    Left,
    Right,
}

/// Right sidebar tab modes (MVP: Find, Feed, Git, Ports, Files, Blocks, Tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RightMode {
    #[default]
    Find,
    Feed,
    Git,
    Ports,
    Files,
    /// GAP-51: block list when `[block_output].enabled`.
    Blocks,
    Toolbelt,
}

impl RightMode {
    pub fn label(self) -> &'static str {
        match self {
            RightMode::Find => "Find",
            RightMode::Feed => "Feed",
            RightMode::Git => "Git",
            RightMode::Ports => "Ports",
            RightMode::Files => "Files",
            RightMode::Blocks => "Blocks",
            RightMode::Toolbelt => "Tools",
        }
    }

    pub fn all() -> &'static [RightMode] {
        &[
            RightMode::Find,
            RightMode::Feed,
            RightMode::Git,
            RightMode::Ports,
            RightMode::Files,
            RightMode::Blocks,
            RightMode::Toolbelt,
        ]
    }

    /// Tabs visible for the current config (Blocks only when block output is on).
    pub fn visible(block_output_enabled: bool) -> Vec<RightMode> {
        Self::all()
            .iter()
            .copied()
            .filter(|m| *m != RightMode::Blocks || block_output_enabled)
            .collect()
    }
}

/// Runtime sidebar visibility + active right mode + per-window widths.
///
/// Widths live here (not on `SidebarConfig`) so each window resizes
/// independently and drag-to-resize can mutate them at runtime; the config
/// table is only the launch default. `Default` starts closed at 240 px.
#[derive(Debug, Clone)]
pub struct SidebarState {
    pub left_open: bool,
    pub right_open: bool,
    pub right_mode: RightMode,
    /// Runtime left-sidebar width (px), clamped to `MIN_WIDTH..=MAX_WIDTH`.
    pub left_width: f32,
    /// Runtime right-sidebar width (px), clamped to `MIN_WIDTH..=MAX_WIDTH`.
    pub right_width: f32,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            left_open: false,
            right_open: false,
            right_mode: RightMode::Find,
            left_width: DEFAULT_WIDTH,
            right_width: DEFAULT_WIDTH,
        }
    }
}

impl SidebarState {
    /// Effective left width: clamped when open, 0 when closed.
    pub fn effective_left_width(&self) -> f32 {
        if self.left_open {
            clamp_width(self.left_width)
        } else {
            0.0
        }
    }

    /// Effective right width: clamped when open, 0 when closed.
    pub fn effective_right_width(&self) -> f32 {
        if self.right_open {
            clamp_width(self.right_width)
        } else {
            0.0
        }
    }

    /// Terminal column width after subtracting open sidebars (≥ 0).
    pub fn terminal_width(&self, viewport_width: f32) -> f32 {
        (viewport_width - self.effective_left_width() - self.effective_right_width()).max(0.0)
    }

    pub fn search_in_sidebar(&self) -> bool {
        self.right_open && self.right_mode == RightMode::Find
    }

    pub fn git_in_sidebar(&self) -> bool {
        self.right_open && self.right_mode == RightMode::Git
    }

    pub fn ports_in_sidebar(&self) -> bool {
        self.right_open && self.right_mode == RightMode::Ports
    }

    pub fn files_in_sidebar(&self) -> bool {
        self.right_open && self.right_mode == RightMode::Files
    }

    pub fn blocks_in_sidebar(&self) -> bool {
        self.right_open && self.right_mode == RightMode::Blocks
    }

    pub fn toolbelt_in_sidebar(&self) -> bool {
        self.right_open && self.right_mode == RightMode::Toolbelt
    }

    #[cfg(unix)]
    pub fn feed_in_sidebar(&self) -> bool {
        self.right_open && self.right_mode == RightMode::Feed
    }
}

#[inline]
pub fn clamp_width(w: f32) -> f32 {
    w.clamp(MIN_WIDTH, MAX_WIDTH)
}

/// GAP-19 review: the EventBus kind emitted for a right-sidebar toggle.
/// Pure decision seam so the Git-mode reopen path can't silently drop the
/// visibility event (review BUG-1) — every toggle emits exactly one of these.
#[inline]
pub fn right_sidebar_event(now_open: bool) -> &'static str {
    if now_open {
        "sidebar.right_opened"
    } else {
        "sidebar.right_closed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_width_bounds() {
        assert_eq!(clamp_width(50.0), MIN_WIDTH);
        assert_eq!(clamp_width(240.0), 240.0);
        assert_eq!(clamp_width(999.0), MAX_WIDTH);
    }

    #[test]
    fn terminal_width_accounts_for_sidebars() {
        let mut st = SidebarState::default();
        st.left_width = 240.0;
        st.right_width = 240.0;
        assert_eq!(st.terminal_width(1000.0), 1000.0);
        st.left_open = true;
        assert_eq!(st.terminal_width(1000.0), 760.0);
        st.right_open = true;
        assert_eq!(st.terminal_width(1000.0), 520.0);
    }

    /// Review BUG-1 regression guard: every right-sidebar toggle emits exactly
    /// one visibility event — the Git-mode reopen path must not skip it.
    #[test]
    fn right_sidebar_event_always_emits() {
        assert_eq!(right_sidebar_event(true), "sidebar.right_opened");
        assert_eq!(right_sidebar_event(false), "sidebar.right_closed");
    }

    /// Runtime widths clamp on construction; drag updates keep the 120–480 bound.
    #[test]
    fn effective_widths_clamp_and_gate_on_open() {
        let mut st = SidebarState::default();
        st.left_width = 50.0; // below MIN
        st.right_width = 999.0; // above MAX
        // Closed → contributes nothing.
        assert_eq!(st.effective_left_width(), 0.0);
        assert_eq!(st.effective_right_width(), 0.0);
        st.left_open = true;
        st.right_open = true;
        assert_eq!(st.effective_left_width(), MIN_WIDTH); // 50 → 120
        assert_eq!(st.effective_right_width(), MAX_WIDTH); // 999 → 480
    }
}
