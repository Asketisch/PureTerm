//! GAP-31: Quake-style dock — slide terminal from screen top via hotkey.

use iced::window;
use iced::{Point, Size, Task};

use crate::iced_app::Message;

/// `[quake]` config.
#[derive(Clone, Debug, PartialEq)]
pub struct QuakeDockConfig {
    pub enabled: bool,
    /// Fraction of screen height (0.0–1.0).
    pub height_fraction: f32,
    pub animation_ms: u32,
}

impl Default for QuakeDockConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            height_fraction: 0.45,
            animation_ms: 120,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct QuakeDockState {
    pub visible: bool,
    /// Saved window position when hidden (for restore).
    pub saved_position: Option<Point>,
    pub saved_size: Option<Size>,
}

impl QuakeDockState {
    pub fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }
}

/// Apply quake show/hide to the main window. MVP: instant resize + y offset
/// (full animation deferred).
pub fn apply_quake_toggle(
    window_id: window::Id,
    state: &mut QuakeDockState,
    cfg: &QuakeDockConfig,
    viewport: Size,
) -> Task<Message> {
    if !cfg.enabled {
        return Task::none();
    }
    let show = state.toggle();
    let h = (viewport.height * cfg.height_fraction.clamp(0.2, 0.95)).max(200.0);
    if show {
        let size = Size::new(viewport.width, h);
        let pos = Point::new(0.0, 0.0);
        Task::batch([
            window::resize(window_id, size),
            window::move_to(window_id, pos),
        ])
    } else if let (Some(pos), Some(size)) = (state.saved_position, state.saved_size) {
        Task::batch([
            window::resize(window_id, size),
            window::move_to(window_id, pos),
        ])
    } else {
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_visibility() {
        let mut st = QuakeDockState::default();
        assert!(st.toggle());
        assert!(!st.toggle());
    }
}
