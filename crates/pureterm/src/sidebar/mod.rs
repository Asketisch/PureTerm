//! GAP-19:双栏侧边栏——左侧(工作区 + Git 摘要)与右侧
//! (Find / Feed / Git 等模式)。状态集中在此,视图在 `left` / `right` 子模块。

pub mod left;
pub mod right;

/// 侧边栏宽度边界(像素)。配置值会被限制在此范围内。
pub const MIN_WIDTH: f32 = 120.0;
pub const MAX_WIDTH: f32 = 480.0;
pub const DEFAULT_WIDTH: f32 = 240.0;

/// 正在拖拽的侧边栏分隔条(GAP-19 拖拽调整宽度)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSide {
    Left,
    Right,
}

/// 右侧侧边栏的标签页模式(MVP:Find、Feed、Git、Ports、Files、Blocks、Tools)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RightMode {
    #[default]
    Find,
    Feed,
    Git,
    Ports,
    Files,
    /// GAP-51:`[block_output].enabled` 启用时的输出块列表。
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

    /// 当前配置下可见的标签页(仅开启输出块时显示 Blocks)。
    pub fn visible(block_output_enabled: bool) -> Vec<RightMode> {
        Self::all()
            .iter()
            .copied()
            .filter(|m| *m != RightMode::Blocks || block_output_enabled)
            .collect()
    }
}

/// 运行时侧边栏可见性 + 当前右侧模式 + 每窗口宽度。
///
/// 宽度保存在这里(而非 `SidebarConfig`),使每个窗口可以独立
/// 缩放,拖拽调整在运行时直接改动这些值;配置
/// 表只是启动时的默认值。`Default` 初始关闭,宽度 240 像素。
#[derive(Debug, Clone)]
pub struct SidebarState {
    pub left_open: bool,
    pub right_open: bool,
    pub right_mode: RightMode,
    /// 运行时左侧侧边栏宽度(像素),限制在 `MIN_WIDTH..=MAX_WIDTH` 内。
    pub left_width: f32,
    /// 运行时右侧侧边栏宽度(像素),限制在 `MIN_WIDTH..=MAX_WIDTH` 内。
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
    /// 生效的左侧宽度:打开时取夹紧值,关闭时为 0。
    pub fn effective_left_width(&self) -> f32 {
        if self.left_open {
            clamp_width(self.left_width)
        } else {
            0.0
        }
    }

    /// 生效的右侧宽度:打开时取夹紧值,关闭时为 0。
    pub fn effective_right_width(&self) -> f32 {
        if self.right_open {
            clamp_width(self.right_width)
        } else {
            0.0
        }
    }

    /// 减去已打开的侧边栏后,终端可用的列宽(≥ 0)。
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

/// GAP-19 review:右侧侧边栏切换时发出的事件总线类型。
/// 作为纯粹的决定性接缝,使 Git 模式重新打开路径不会悄悄丢弃
/// 可见性事件(review BUG-1)——每次切换恰好发出其一。
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

    /// Review BUG-1 回归防护:每次右侧侧边栏切换恰好发出
    /// 一个可见性事件——Git 模式重新打开路径不得跳过它。
    #[test]
    fn right_sidebar_event_always_emits() {
        assert_eq!(right_sidebar_event(true), "sidebar.right_opened");
        assert_eq!(right_sidebar_event(false), "sidebar.right_closed");
    }

    /// 构造时对运行时宽度做夹紧;拖拽更新保持在 120–480 边界内。
    #[test]
    fn effective_widths_clamp_and_gate_on_open() {
        let mut st = SidebarState::default();
        st.left_width = 50.0; // 低于最小值
        st.right_width = 999.0; // 高于最大值
        // 关闭 → 不贡献任何宽度。
        assert_eq!(st.effective_left_width(), 0.0);
        assert_eq!(st.effective_right_width(), 0.0);
        st.left_open = true;
        st.right_open = true;
        assert_eq!(st.effective_left_width(), MIN_WIDTH); // 50 → 120
        assert_eq!(st.effective_right_width(), MAX_WIDTH); // 999 → 480
    }
}
