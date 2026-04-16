//! Quake mode — dropdown terminal that slides from the top of the screen.

use winit::window::Window;

/// Quake mode configuration and state.
#[allow(dead_code)]
pub struct QuakeMode {
    /// Whether quake mode is enabled.
    pub enabled: bool,
    /// Whether the quake window is currently visible.
    pub visible: bool,
    /// Height of the quake window as a fraction of screen height (0.0-1.0).
    pub height: f32,
    /// Animation duration in milliseconds.
    pub animation_ms: u32,
    /// Screen index to show the quake window on (0-based).
    pub screen_index: usize,
    /// Whether to always stay on top.
    pub always_on_top: bool,
}

impl Default for QuakeMode {
    fn default() -> Self {
        Self {
            enabled: false,
            visible: false,
            height: 0.5,
            animation_ms: 150,
            screen_index: 0,
            always_on_top: true,
        }
    }
}

impl QuakeMode {
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle the quake window visibility.
    pub fn toggle(&mut self) -> QuakeAction {
        if self.visible {
            self.visible = false;
            QuakeAction::Hide
        } else {
            self.visible = true;
            QuakeAction::Show
        }
    }

    /// Show the quake window.
    #[allow(dead_code)]
    pub fn show(&mut self) -> QuakeAction {
        self.visible = true;
        QuakeAction::Show
    }

    /// Hide the quake window.
    #[allow(dead_code)]
    pub fn hide(&mut self) -> QuakeAction {
        self.visible = false;
        QuakeAction::Hide
    }

    /// Apply quake mode window attributes to a winit Window.
    #[allow(dead_code)]
    pub fn apply_window_attrs(&self, window: &Window) {
        if !self.enabled {
            return;
        }

        // Set always on top
        // window.set_always_on_top(self.always_on_top); // winit may not support this directly

        // Position at top of screen
        if let Some(monitor) = window.available_monitors().nth(self.screen_index) {
            let screen_size = monitor.size();
            let quake_height = (screen_size.height as f32 * self.height) as u32;
            let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(
                screen_size.width,
                quake_height,
            ));
            let _ = window.set_outer_position(winit::dpi::PhysicalPosition::new(
                monitor.position().x,
                monitor.position().y,
            ));
        }
    }

    /// Whether the window should be shown (for the event loop to decide).
    #[allow(dead_code)]
    pub fn should_show(&self) -> bool {
        self.enabled && self.visible
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuakeAction {
    Show,
    Hide,
}
