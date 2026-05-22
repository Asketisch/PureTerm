struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) color: vec4f,
}

struct ScreenUniforms {
    corner_radius: f32,
    screen_width: f32,
    screen_height: f32,
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
    out.position = vec4f(position, 0.0, 1.0);
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
