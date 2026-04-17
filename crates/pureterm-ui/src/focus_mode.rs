//! 专注模式 — 隐藏除终端内容外的所有 UI 界面。


/// 管理专注模式状态。
pub struct FocusMode {
    pub active: bool,
    /// 进入专注模式前记住哪些是可见的。
    was_tab_bar_visible: bool,
    was_title_bar_visible: bool,
    was_sidebar_visible: bool,
}

impl Default for FocusMode {
    fn default() -> Self {
        Self {
            active: false,
            was_tab_bar_visible: true,
            was_title_bar_visible: true,
            was_sidebar_visible: false,
        }
    }
}

impl FocusMode {
    pub fn new() -> Self {
        Self::default()
    }

    /// 切换专注模式的开关。
    pub fn toggle(&mut self, tab_bar_visible: bool, title_bar_visible: bool, sidebar_visible: bool) {
        if !self.active {
            // 进入专注模式 — 保存当前状态
            self.was_tab_bar_visible = tab_bar_visible;
            self.was_title_bar_visible = title_bar_visible;
            self.was_sidebar_visible = sidebar_visible;
            self.active = true;
        } else {
            // 退出专注模式
            self.active = false;
        }
    }

    /// 标签栏是否应该可见。
    pub fn show_tab_bar(&self, original: bool) -> bool {
        if self.active { false } else { original }
    }

    /// 标题栏是否应该可见。
    pub fn show_title_bar(&self, original: bool) -> bool {
        if self.active { false } else { original }
    }

    /// 侧边栏是否应该可见。
    pub fn show_sidebar(&self, original: bool) -> bool {
        if self.active { false } else { original }
    }

    /// 是否在专注模式显示命令面板切换提示。
    pub fn show_focus_hint(&self) -> bool {
        self.active
    }

    /// 恢复状态(退出专注模式后调用)。
    pub fn restore_tab_bar(&self) -> bool {
        self.was_tab_bar_visible
    }

    pub fn restore_title_bar(&self) -> bool {
        self.was_title_bar_visible
    }

    pub fn restore_sidebar(&self) -> bool {
        self.was_sidebar_visible
    }
}
