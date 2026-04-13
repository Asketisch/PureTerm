use std::time::Instant;

/// A single tab in the terminal.
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

/// Content of a tab - either a single pane or a split container.
#[derive(Debug)]
pub enum TabContent {
    Pane(u64),
    Split(SplitRef),
}

/// Reference to a split container.
#[derive(Debug)]
pub struct SplitRef {
    pub root_id: u64,
}

/// Record of a closed tab for restoration.
#[derive(Debug, Clone)]
pub struct ClosedTab {
    pub id: u64,
    pub title: String,
    pub closed_at: Instant,
}

/// Manages open tabs.
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

    /// Open a new tab with the given content.
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

    /// Close a tab by ID.
    pub fn close_tab(&mut self, id: u64) -> Option<ClosedTab> {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            let tab = self.tabs.remove(pos);
            let closed = ClosedTab {
                id: tab.id,
                title: tab.title,
                closed_at: Instant::now(),
            };
            self.closed_tabs_stack.push(closed.clone());

            // Update active tab
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

    /// Close the currently active tab.
    pub fn close_active_tab(&mut self) -> Option<ClosedTab> {
        if let Some(id) = self.active_tab {
            self.close_tab(id)
        } else {
            None
        }
    }

    /// Select a tab by ID.
    pub fn select_tab(&mut self, id: u64) {
        if self.tabs.iter().any(|t| t.id == id) {
            // Remove focus from all tabs
            for tab in &mut self.tabs {
                tab.has_focus = false;
                tab.has_activity = false;
            }
            // Set focus on the selected tab
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                tab.has_focus = true;
            }
            self.active_tab = Some(id);
        }
    }

    /// Switch to the previous tab.
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

    /// Switch to the next tab.
    pub fn next_tab(&mut self) {
        if let Some(active_id) = self.active_tab {
            if let Some(pos) = self.tabs.iter().position(|t| t.id == active_id) {
                let next_pos = (pos + 1) % self.tabs.len();
                self.select_tab(self.tabs[next_pos].id);
            }
        }
    }

    /// Get the active tab.
    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab.and_then(|id| self.tabs.iter().find(|t| t.id == id))
    }

    /// Get the active tab mutably.
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.active_tab.and_then(move |id| self.tabs.iter_mut().find(|t| t.id == id))
    }

    /// Get all tabs.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Get the number of open tabs.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Check if there are no tabs.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Get the recently closed tabs stack.
    pub fn closed_tabs(&self) -> &[ClosedTab] {
        &self.closed_tabs_stack
    }

    /// Restore the most recently closed tab.
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

    /// Set the title of a tab.
    pub fn set_title(&mut self, id: u64, title: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.title = title;
        }
    }

    /// Set the color of a tab.
    pub fn set_color(&mut self, id: u64, color: Option<String>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.color = color;
        }
    }

    /// Mark a tab as having activity.
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
