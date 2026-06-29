//! GAP-19: Dual Sidebar — left (Workspace + Git summary) and right
//! (Find / Feed / Git modes). State lives here; views in `left` / `right`.

pub mod left;
pub mod right;

/// Sidebar width bounds (px). Config values are clamped to this range.
pub const MIN_WIDTH: f32 = 120.0;
pub const MAX_WIDTH: f32 = 480.0;
pub const DEFAULT_WIDTH: f32 = 240.0;

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

/// Runtime sidebar visibility + active right mode.
#[derive(Debug, Clone, Default)]
pub struct SidebarState {
    pub left_open: bool,
    pub right_open: bool,
    pub right_mode: RightMode,
}

impl SidebarState {
    pub fn left_width(&self, cfg_width: f32) -> f32 {
        if self.left_open {
            clamp_width(cfg_width)
        } else {
            0.0
        }
    }

    pub fn right_width(&self, cfg_width: f32) -> f32 {
        if self.right_open {
            clamp_width(cfg_width)
        } else {
            0.0
        }
    }

    /// Terminal column width after subtracting open sidebars.
    pub fn terminal_width(&self, viewport_width: f32, left_cfg: f32, right_cfg: f32) -> f32 {
        (viewport_width - self.left_width(left_cfg) - self.right_width(right_cfg)).max(0.0)
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
        assert_eq!(st.terminal_width(1000.0, 240.0, 240.0), 1000.0);
        st.left_open = true;
        assert_eq!(st.terminal_width(1000.0, 240.0, 240.0), 760.0);
        st.right_open = true;
        assert_eq!(st.terminal_width(1000.0, 240.0, 240.0), 520.0);
    }
}
