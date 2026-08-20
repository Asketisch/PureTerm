//! GAP-24:经 `iced::widget::canvas` 绘制的迷你走势图。
//!
//! 移植了右锚定的迷你走势图曲线算法:右锚定折线
//! (最新样本位于右缘),天花板自动缩放(下限 1.0,
//! 使全零序列仍可渲染而不会除零)。固定
//! 60 槽窗口;样本较少时占据右侧部分。

use iced::mouse;
use iced::widget::canvas::{self, Geometry};
use iced::{Color, Point, Rectangle};

/// 可见样本窗口(与主流终端迷你走势图的窗口大小一致)。
pub const WINDOW: usize = 60;

/// 渲染单个时间序列迷你走势图的自包含 canvas 程序。
#[derive(Clone)]
pub struct SparklineProgram {
    /// 最近的样本在后。为空或少于 2 个 ⇒ 不绘制任何内容。
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
            // 自动缩放天花板:取最大样本,但绝不低于 1.0。
            let ceiling = self.samples.iter().copied().fold(0.0f32, f32::max).max(1.0);

            // 右锚定:最后一个样本位于右缘。
            let x_for = |i: usize| w - (n - 1 - i) as f32 * bar_w;
            // y 轴向下增长;值 0 → 底部,值=ceiling → 顶部。
            let y_for = |v: f32| (h - (v / ceiling) * h).clamp(0.0, h);

            // 折线点只收集一次;`canvas::path::Builder` 不是
            // `Clone`,因此描边(开放折线)与填充(闭合多边形)
            // 由共享这些点的两个独立 builder 分别构建。
            let pts: Vec<Point> = (0..n)
                .map(|i| Point::new(x_for(i), y_for(self.samples[i])))
                .collect();

            let mut bs = canvas::path::Builder::new();
            bs.move_to(pts[0]);
            for p in pts.iter().skip(1) {
                bs.line_to(*p);
            }
            let stroke = bs.build();

            // 把多边形闭合到基线,形成半透明填充。
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
