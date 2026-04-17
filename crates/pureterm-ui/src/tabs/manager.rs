use std::time::Instant;

/// 终端中的单个标签页。
#[derive(Debug)]
pub struct Tab {
    pub id: u64,
    pub title: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub content: TabContent,
    pub has_activity: bool,
    pub has_focus: bool,
    pub created_at: Instant,
}

/// 标签页内容 - 单个面板或分割容器。
#[derive(Debug)]
pub enum TabContent {
    Pane(u64),
    Split(SplitRef),
}

/// 分割容器的引用。
#[derive(Debug)]
pub struct SplitRef {
    pub root_id: u64,
}

/// 已关闭标签页的记录,用于恢复。
#[derive(Debug, Clone)]
pub struct ClosedTab {
    pub id: u64,
    pub title: String,
    pub closed_at: Instant,
}

/// 管理打开的标签页。
pub struct TabManager {
    tabs: Vec<Tab>,
    active_tab: Option<u64>,
    closed_tabs_stack: Vec<ClosedTab>,
    next_id: u64,
}

impl TabManager {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            closed_tabs_stack: Vec::new(),
            next_id: 1,
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// 使用给定内容打开新标签页。
    pub fn open_tab(&mut self, content: TabContent, title: String) -> u64 {
        let id = self.alloc_id();
        let tab = Tab {
            id,
            title,
            icon: None,
            color: None,
            content,
            has_activity: false,
            has_focus: false,
            created_at: Instant::now(),
        };
        self.tabs.push(tab);
        self.active_tab = Some(id);
        id
    }

    /// 通过 ID 关闭标签页。
    pub fn close_tab(&mut self, id: u64) -> Option<ClosedTab> {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            let tab = self.tabs.remove(pos);
            let closed = ClosedTab {
                id: tab.id,
                title: tab.title,
                closed_at: Instant::now(),
            };
            self.closed_tabs_stack.push(closed.clone());

            // 更新活动标签页
            if self.active_tab == Some(id) {
                self.active_tab = if self.tabs.is_empty() {
                    None
                } else {
                    Some(self.tabs[pos.min(self.tabs.len() - 1)].id)
                };
            }

            return Some(closed);
        }
        None
    }

    /// 关闭当前活动的标签页。
    pub fn close_active_tab(&mut self) -> Option<ClosedTab> {
        if let Some(id) = self.active_tab {
            self.close_tab(id)
        } else {
            None
        }
    }

    /// 通过 ID 选择标签页。
    pub fn select_tab(&mut self, id: u64) {
        if self.tabs.iter().any(|t| t.id == id) {
            // 从所有标签页移除焦点
            for tab in &mut self.tabs {
                tab.has_focus = false;
                tab.has_activity = false;
            }
            // 在选定的标签页上设置焦点
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                tab.has_focus = true;
            }
            self.active_tab = Some(id);
        }
    }

    /// 切换到上一个标签页。
    pub fn previous_tab(&mut self) {
        if let Some(active_id) = self.active_tab {
            if let Some(pos) = self.tabs.iter().position(|t| t.id == active_id) {
                let prev_pos = if pos == 0 {
                    self.tabs.len() - 1
                } else {
                    pos - 1
                };
                self.select_tab(self.tabs[prev_pos].id);
            }
        }
    }

    /// 切换到下一个标签页。
    pub fn next_tab(&mut self) {
        if let Some(active_id) = self.active_tab {
            if let Some(pos) = self.tabs.iter().position(|t| t.id == active_id) {
                let next_pos = (pos + 1) % self.tabs.len();
                self.select_tab(self.tabs[next_pos].id);
            }
        }
    }

    /// 获取活动的标签页。
    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab.and_then(|id| self.tabs.iter().find(|t| t.id == id))
    }

    /// 获取活动标签页的可变引用。
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.active_tab.and_then(move |id| self.tabs.iter_mut().find(|t| t.id == id))
    }

    /// 获取所有标签页。
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// 获取打开的标签页数量。
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// 检查是否没有标签页。
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// 获取最近关闭的标签页栈。
    pub fn closed_tabs(&self) -> &[ClosedTab] {
        &self.closed_tabs_stack
    }

    /// 恢复最近关闭的标签页。
    pub fn restore_last_closed(&mut self) -> Option<u64> {
        let closed = self.closed_tabs_stack.pop()?;
        let pane_id = self.alloc_id();
        let title = closed.title;
        let id = self.open_tab(
            TabContent::Pane(pane_id),
            title,
        );
        Some(id)
    }

    /// 设置标签页的标题。
    pub fn set_title(&mut self, id: u64, title: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.title = title;
        }
    }

    /// 设置标签页的颜色。
    pub fn set_color(&mut self, id: u64, color: Option<String>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.color = color;
        }
    }

    /// 将标签页标记为有活动。
    pub fn mark_activity(&mut self, id: u64) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            if !tab.has_focus {
                tab.has_activity = true;
            }
        }
    }
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}
