//! PureTerm 的多窗口支持。

use std::collections::HashMap;
use winit::window::WindowId;

/// 管理多个终端窗口。
pub struct MultiWindowManager {
    windows: HashMap<WindowId, WindowState>,
    active_window: Option<WindowId>,
}

#[derive(Debug, Clone)]
pub struct WindowState {
    pub id: WindowId,
    pub title: String,
    pub has_focus: bool,
    pub is_quake: bool,
}

impl MultiWindowManager {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            active_window: None,
        }
    }

    /// 注册新窗口。
    pub fn register(&mut self, id: WindowId, title: &str) {
        self.windows.insert(id, WindowState {
            id,
            title: title.to_string(),
            has_focus: false,
            is_quake: false,
        });
        self.active_window = Some(id);
    }

    /// 注销窗口(当它关闭时)。
    pub fn unregister(&mut self, id: WindowId) {
        self.windows.remove(&id);
        if self.active_window == Some(id) {
            self.active_window = self.windows.keys().next().copied();
        }
    }

    /// 设置活动(聚焦)窗口。
    pub fn set_active(&mut self, id: WindowId) {
        if let Some(active) = self.active_window {
            if let Some(state) = self.windows.get_mut(&active) {
                state.has_focus = false;
            }
        }
        if let Some(state) = self.windows.get_mut(&id) {
            state.has_focus = true;
        }
        self.active_window = Some(id);
    }

    /// 将窗口标记为 quake 窗口。
    pub fn set_quake(&mut self, id: WindowId, is_quake: bool) {
        if let Some(state) = self.windows.get_mut(&id) {
            state.is_quake = is_quake;
        }
    }

    /// 获取活动窗口 ID。
    pub fn active_window(&self) -> Option<WindowId> {
        self.active_window
    }

    /// 获取窗口的状态。
    pub fn get(&self, id: WindowId) -> Option<&WindowState> {
        self.windows.get(&id)
    }

    /// 打开窗口的数量。
    pub fn count(&self) -> usize {
        self.windows.len()
    }

    /// 是否存在任何窗口。
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// 获取 quake 窗口(如果存在)。
    pub fn quake_window(&self) -> Option<WindowId> {
        self.windows.values()
            .find(|w| w.is_quake)
            .map(|w| w.id)
    }

    /// 列出所有窗口。
    pub fn windows(&self) -> &HashMap<WindowId, WindowState> {
        &self.windows
    }
}

impl Default for MultiWindowManager {
    fn default() -> Self {
        Self::new()
    }
}
