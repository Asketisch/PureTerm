//! Iced 的圆形 HSV 色轮组件。
//! 基于 nickbrannigan 的 iced_color_wheel 改写(BSD-2 许可证)

use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Event, Point, Rectangle, Renderer, Size, Theme};
use std::cell::Cell;
use std::f32::consts::PI;

// 渲染分辨率
const HUE_STEPS: usize = 360;
const SAT_STEPS: usize = 128;

// ---------------------------------------------------------------------------
// HSV <-> RGB 辅助函数
// ---------------------------------------------------------------------------

/// 把 HSV(h: 0-360,s: 0-1,v: 0-1)转换为 iced::Color。
pub fn hsv_to_color(h: f32, s: f32, v: f32) -> Color {
    let h = ((h % 360.0) + 360.0) % 360.0;
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);

    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    Color::from_rgb(r + m, g + m, b + m)
}

/// 把 iced::Color 转换为 HSV(h: 0-360,s: 0-1,v: 0-1)。
pub fn color_to_hsv(c: Color) -> (f32, f32, f32) {
    let r = c.r;
    let g = c.g;
    let b = c.b;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;

    let v = max;
    let s = if max.abs() < 1e-6 { 0.0 } else { d / max };

    let h = if d.abs() < 1e-6 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / d) % 6.0)
    } else if (max - g).abs() < 1e-6 {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };

    let h = ((h % 360.0) + 360.0) % 360.0;
    (h, s, v)
}

/// 把 HSV 转换为 hex 字符串(#RRGGBB)。
pub fn hsv_to_hex(h: f32, s: f32, v: f32) -> String {
    let c = hsv_to_color(h, s, v);
    format!(
        "#{:02X}{:02X}{:02X}",
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
    )
}

/// 解析 hex 字符串(#RRGGBB 或 RRGGBB)为 iced::Color。
pub fn hex_to_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    // 按字节判长后切片要求内容为 ASCII,否则 CJK 输入(两个汉字恰好
    // 6 字节)会在非字符边界切片 panic——用户敲一半的输入即可触发。
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

// ---------------------------------------------------------------------------
// Canvas 程序——色轮渲染 + 交互
// ---------------------------------------------------------------------------

/// Iced 的圆形 HSV 色轮 canvas 程序。
///
/// 对任意消息类型泛型。提供一个把 `(hue, saturation)`
/// 映射到你的应用消息类型的回调。
pub struct WheelProgram<Message> {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
    on_change: Box<dyn Fn(f32, f32) -> Message>,
}

impl<Message> WheelProgram<Message> {
    /// 创建新的色轮程序。
    pub fn new(
        hue: f32,
        saturation: f32,
        value: f32,
        on_change: impl Fn(f32, f32) -> Message + 'static,
    ) -> Self {
        Self {
            hue,
            saturation,
            value,
            on_change: Box::new(on_change),
        }
    }
}

/// 内部 canvas 状态(跨帧保留)。
pub struct WheelState {
    is_dragging: bool,
    wheel_cache: canvas::Cache,
    last_value: Cell<f32>,
}

impl Default for WheelState {
    fn default() -> Self {
        Self {
            is_dragging: false,
            wheel_cache: canvas::Cache::new(),
            last_value: Cell::new(f32::NAN),
        }
    }
}

impl<Message: Clone> canvas::Program<Message> for WheelProgram<Message> {
    type State = WheelState;

    fn update(
        &self,
        state: &mut WheelState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    if let Some((h, s)) = wheel_hit_test(pos, bounds.size()) {
                        state.is_dragging = true;
                        return Some(Action::publish((self.on_change)(h, s)).and_capture());
                    }
                }
                None
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.is_dragging {
                    if let Some(pos) = cursor.position_in(bounds) {
                        let (h, s) = wheel_position_to_hs(pos, bounds.size());
                        return Some(Action::publish((self.on_change)(h, s)).and_capture());
                    }
                }
                None
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.is_dragging {
                    state.is_dragging = false;
                    return Some(Action::capture());
                }
                None
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &WheelState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        // 亮度变化时使色轮缓存失效
        if state.last_value.get() != self.value {
            state.wheel_cache.clear();
            state.last_value.set(self.value);
        }

        // 绘制色轮(有缓存——仅在明度或尺寸变化时重算)
        let wheel = state.wheel_cache.draw(renderer, bounds.size(), |frame| {
            draw_wheel(frame, bounds.size(), self.value);
        });

        // 绘制选择器(轻量——仅 2 个圆,总是新画)
        let mut selector_frame = Frame::new(renderer, bounds.size());
        draw_selector(
            &mut selector_frame,
            bounds.size(),
            self.hue,
            self.saturation,
        );

        vec![wheel, selector_frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &WheelState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.is_dragging {
            return mouse::Interaction::Grabbing;
        }
        if let Some(pos) = cursor.position_in(bounds) {
            if wheel_hit_test(pos, bounds.size()).is_some() {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::default()
    }
}

// ---------------------------------------------------------------------------
// 几何辅助
// ---------------------------------------------------------------------------

fn wheel_geometry(size: Size) -> (f32, f32, f32) {
    let side = size.width.min(size.height);
    let radius = side / 2.0 - 4.0;
    let cx = size.width / 2.0;
    let cy = size.height / 2.0;
    (cx, cy, radius)
}

fn wheel_hit_test(pos: Point, size: Size) -> Option<(f32, f32)> {
    let (cx, cy, radius) = wheel_geometry(size);
    let dx = pos.x - cx;
    let dy = pos.y - cy;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist <= radius {
        let hue = (dy.atan2(dx).to_degrees() + 360.0) % 360.0;
        let sat = (dist / radius).min(1.0);
        Some((hue, sat))
    } else {
        None
    }
}

fn wheel_position_to_hs(pos: Point, size: Size) -> (f32, f32) {
    let (cx, cy, radius) = wheel_geometry(size);
    let dx = pos.x - cx;
    let dy = pos.y - cy;
    let hue = (dy.atan2(dx).to_degrees() + 360.0) % 360.0;
    let dist = (dx * dx + dy * dy).sqrt();
    let sat = (dist / radius).clamp(0.0, 1.0);
    (hue, sat)
}

// ---------------------------------------------------------------------------
// 绘制
// ---------------------------------------------------------------------------

fn draw_wheel(frame: &mut Frame, size: Size, value: f32) {
    let (cx, cy, radius) = wheel_geometry(size);

    let angle_step = 2.0 * PI / HUE_STEPS as f32;
    let r_step = radius / SAT_STEPS as f32;

    for h_idx in 0..HUE_STEPS {
        let a0 = h_idx as f32 * angle_step;
        let a1 = (h_idx + 1) as f32 * angle_step;
        let hue = h_idx as f32 * (360.0 / HUE_STEPS as f32);

        let cos0 = a0.cos();
        let sin0 = a0.sin();
        let cos1 = a1.cos();
        let sin1 = a1.sin();

        for s_idx in 0..SAT_STEPS {
            let r_inner = s_idx as f32 * r_step;
            let r_outer = (s_idx + 1) as f32 * r_step;
            let sat = (s_idx as f32 + 0.5) / SAT_STEPS as f32;

            let color = hsv_to_color(hue, sat, value);

            let path = Path::new(|b| {
                b.move_to(Point::new(cx + r_inner * cos0, cy + r_inner * sin0));
                b.line_to(Point::new(cx + r_inner * cos1, cy + r_inner * sin1));
                b.line_to(Point::new(cx + r_outer * cos1, cy + r_outer * sin1));
                b.line_to(Point::new(cx + r_outer * cos0, cy + r_outer * sin0));
                b.close();
            });

            frame.fill(&path, color);
        }
    }

    let border = Path::circle(Point::new(cx, cy), radius);
    frame.stroke(
        &border,
        Stroke::default()
            .with_color(Color::from_rgb(0.4, 0.4, 0.4))
            .with_width(1.0),
    );
}

fn draw_selector(frame: &mut Frame, size: Size, hue: f32, saturation: f32) {
    let (cx, cy, radius) = wheel_geometry(size);

    let angle = hue.to_radians();
    let dist = saturation * radius;
    let sx = cx + dist * angle.cos();
    let sy = cy + dist * angle.sin();

    let outer = Path::circle(Point::new(sx, sy), 7.0);
    frame.stroke(
        &outer,
        Stroke::default()
            .with_color(Color::from_rgb(0.1, 0.1, 0.1))
            .with_width(2.0),
    );

    let inner = Path::circle(Point::new(sx, sy), 5.0);
    frame.stroke(
        &inner,
        Stroke::default()
            .with_color(Color::WHITE)
            .with_width(2.0),
    );
}
