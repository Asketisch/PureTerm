struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) color: vec4f,
}

struct ScreenUniforms {
    corner_radius: f32,
    screen_width: f32,
    screen_height: f32,
    /// 背景图不透明度(0 = 无图像)。
    opacity: f32,
    /// 0 = 居中,1 = 平铺,2 = 拉伸。
    mode: u32,
    /// 图像原生像素尺寸(用于居中计算)。
    img_width: f32,
    img_height: f32,
    /// 尾部填充:把结构体补到 32 字节,与 Rust 侧的
    /// `ScreenUniforms` 逐字段一致。四个 shader 共用同一份
    /// uniform 缓冲区,布局必须完全相同。
    _padding: f32,
}

@group(0) @binding(0)
var<uniform> u_screen: ScreenUniforms;

@vertex
fn vs_main(
    @location(0) position: vec2f,
    @location(1) color: vec4f,
) -> VertexOutput {
    var out: VertexOutput;
    let ndc_x = position.x / u_screen.screen_width * 2.0 - 1.0;
    let ndc_y = 1.0 - position.y / u_screen.screen_height * 2.0;
    out.position = vec4f(ndc_x, ndc_y, 0.0, 1.0);
    out.color = color;
    return out;
}

fn is_in_rounded_corner(pos: vec2f, corner: vec2f, radius: f32) -> bool {
    if (radius <= 0.0) {
        return true;
    }
    let d = distance(pos, corner);
    return d <= radius;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    let radius = u_screen.corner_radius;
    if (radius > 0.0) {
        let screen_w = u_screen.screen_width;
        let screen_h = u_screen.screen_height;

        let pixel_x = in.position.x;
        let pixel_y = in.position.y;

        if (pixel_x < radius && pixel_y < radius) {
            if (!is_in_rounded_corner(vec2f(pixel_x, pixel_y), vec2f(radius, radius), radius)) {
                discard;
            }
        }
        if (pixel_x > screen_w - radius && pixel_y < radius) {
            if (!is_in_rounded_corner(vec2f(pixel_x, pixel_y), vec2f(screen_w - radius, radius), radius)) {
                discard;
            }
        }
        if (pixel_x < radius && pixel_y > screen_h - radius) {
            if (!is_in_rounded_corner(vec2f(pixel_x, pixel_y), vec2f(radius, screen_h - radius), radius)) {
                discard;
            }
        }
        if (pixel_x > screen_w - radius && pixel_y > screen_h - radius) {
            if (!is_in_rounded_corner(vec2f(pixel_x, pixel_y), vec2f(screen_w - radius, screen_h - radius), radius)) {
                discard;
            }
        }
    }

    return in.color;
}
