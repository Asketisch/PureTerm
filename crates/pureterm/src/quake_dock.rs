//! GAP-31:Quake 风格停靠——热键从屏幕顶端滑出终端。

use iced::window;
use iced::{Point, Size, Task};

use crate::iced_app::Message;

/// `[quake]` 配置。
#[derive(Clone, Debug, PartialEq)]
pub struct QuakeDockConfig {
    pub enabled: bool,
    /// 占屏幕高度的比例(0.0–1.0)。
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
    /// 隐藏时保存的窗口位置(用于恢复)。
    pub saved_position: Option<Point>,
    pub saved_size: Option<Size>,
}

impl QuakeDockState {
    pub fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }
}

/// 对主窗口应用 quake 显示/隐藏。MVP:即时调整尺寸 + y 偏移
///(完整动画留待后续)。
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
