//! GAP-24: sparkline mini-graph drawn via `iced::widget::canvas`.
//!
//! Ports a right-anchored sparkline bezier math: right-anchored polyline
//! (newest sample at the right edge), auto-scaling ceiling (floored at 1.0
//! so an all-zero series still renders without divide-by-zero). Fixed
//! 60-slot window; fewer samples occupy the right portion.

use iced::mouse;
use iced::widget::canvas::{self, Geometry};
use iced::{Color, Point, Rectangle};

/// Visible sample window (matches the mainstream terminal sparkline window size).
pub const WINDOW: usize = 60;

/// A self-contained canvas program rendering one time-series sparkline.
#[derive(Clone)]
pub struct SparklineProgram {
    /// Most-recent-last samples. Empty/fewer-than-2 ⇒ nothing drawn.
    pub samples: Vec<f32>,
    pub color: Color,
    pub fill_color: Color,
}

impl canvas::Program<crate::iced_app::Message> for SparklineProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<iced::Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let n = self.samples.len();
        if n >= 2 {
            let w = frame.width();
            let h = frame.height();
            let bar_w = w / ((WINDOW - 1).max(1) as f32);
            // Auto-scaling ceiling: largest sample, never below 1.0.
            let ceiling = self.samples.iter().copied().fold(0.0f32, f32::max).max(1.0);

            // Right-anchored: last sample sits at the right edge.
            let x_for = |i: usize| w - (n - 1 - i) as f32 * bar_w;
            // y grows downward; value 0 → bottom, value=ceiling → top.
            let y_for = |v: f32| (h - (v / ceiling) * h).clamp(0.0, h);

            // Collect the polyline points once; `canvas::path::Builder` isn't
            // `Clone`, so the stroke (open polyline) and fill (closed polygon)
            // are built from two separate builders sharing these points.
            let pts: Vec<Point> = (0..n)
                .map(|i| Point::new(x_for(i), y_for(self.samples[i])))
                .collect();

            let mut bs = canvas::path::Builder::new();
            bs.move_to(pts[0]);
            for p in pts.iter().skip(1) {
                bs.line_to(*p);
            }
            let stroke = bs.build();

            // Close the polygon down to the baseline for a translucent fill.
            let mut bf = canvas::path::Builder::new();
            bf.move_to(pts[0]);
            for p in pts.iter().skip(1) {
                bf.line_to(*p);
            }
            bf.line_to(Point::new(pts[n - 1].x, h));
            bf.line_to(Point::new(pts[0].x, h));
            bf.close();
            frame.fill(&bf.build(), self.fill_color);
            frame.stroke(
                &stroke,
                canvas::Stroke::default().with_width(1.0).with_color(self.color),
            );
        }
        vec![frame.into_geometry()]
    }
}
