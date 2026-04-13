//! egui integration layer for PureTerm.
//!
//! Provides an egui widget that reserves space for terminal content
//! and handles mouse events. The actual terminal rendering is done
//! via the existing wgpu text pipeline.

use egui::{Color32, Response, Sense, Ui, Vec2, Widget};
use pureterm_core::theme::ColorScheme;
use pureterm_terminal::ScreenSnapshot;

/// egui widget that displays terminal content.
///
/// This widget reserves space in the egui layout and handles mouse events.
/// The actual terminal rendering is done via the wgpu text pipeline
/// in the main render loop.
pub struct TerminalWidget<'a> {
    snapshot: &'a ScreenSnapshot,
    color_scheme: &'a ColorScheme,
    /// Requested size in pixels.
    size_pixels: (f32, f32),
    /// Whether the terminal has focus.
    focused: bool,
}

impl<'a> TerminalWidget<'a> {
    pub fn new(
        snapshot: &'a ScreenSnapshot,
        color_scheme: &'a ColorScheme,
        size_pixels: (f32, f32),
    ) -> Self {
        Self {
            snapshot,
            color_scheme,
            size_pixels,
            focused: false,
        }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

impl<'a> Widget for TerminalWidget<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        let desired_size = Vec2::new(self.size_pixels.0, self.size_pixels.1);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());

        // Draw background
        let bg_color = ColorScheme::parse_color(&self.color_scheme.background);
        let bg = Color32::from_rgb(bg_color[0], bg_color[1], bg_color[2]);
        ui.painter().rect_filled(rect, 0.0, bg);

        // Draw focus indicator
        if self.focused {
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::ZERO,
                egui::Stroke::new(1.0, Color32::from_rgb(100, 100, 255)),
                egui::StrokeKind::Outside,
            );
        }

        response
    }
}
