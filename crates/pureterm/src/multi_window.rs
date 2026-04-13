//! Multi-window support for PureTerm.

use std::collections::HashMap;
use winit::window::WindowId;

/// Manages multiple terminal windows.
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

    /// Register a new window.
    pub fn register(&mut self, id: WindowId, title: &str) {
        self.windows.insert(id, WindowState {
            id,
            title: title.to_string(),
            has_focus: false,
            is_quake: false,
        });
        self.active_window = Some(id);
    }

    /// Unregister a window (when it closes).
    pub fn unregister(&mut self, id: WindowId) {
        self.windows.remove(&id);
        if self.active_window == Some(id) {
            self.active_window = self.windows.keys().next().copied();
        }
    }

    /// Set the active (focused) window.
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

    /// Mark a window as the quake window.
    pub fn set_quake(&mut self, id: WindowId, is_quake: bool) {
        if let Some(state) = self.windows.get_mut(&id) {
            state.is_quake = is_quake;
        }
    }

    /// Get the active window ID.
    pub fn active_window(&self) -> Option<WindowId> {
        self.active_window
    }

    /// Get a window's state.
    pub fn get(&self, id: WindowId) -> Option<&WindowState> {
        self.windows.get(&id)
    }

    /// Number of open windows.
    pub fn count(&self) -> usize {
        self.windows.len()
    }

    /// Whether there are any windows.
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Get the quake window if one exists.
    pub fn quake_window(&self) -> Option<WindowId> {
        self.windows.values()
            .find(|w| w.is_quake)
            .map(|w| w.id)
    }

    /// List all windows.
    pub fn windows(&self) -> &HashMap<WindowId, WindowState> {
        &self.windows
    }
}

impl Default for MultiWindowManager {
    fn default() -> Self {
        Self::new()
    }
}
