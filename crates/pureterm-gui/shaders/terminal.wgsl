struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) tex_coords: vec2f,
    @location(1) color: vec4f,
}

/// 终端渲染 Uniforms
struct TerminalUniforms {
    corner_radius: f32,   // 圆角半径（像素）
    screen_width: f32,   // 屏幕宽度
    screen_height: f32,  // 屏幕高度
}

@group(0) @binding(0)
var u_texture: texture_2d<f32>;

@group(0) @binding(1)
var u_sampler: sampler;

@group(0) @binding(2)
var<uniform> u_uniforms: TerminalUniforms;

@vertex
fn vs_main(
    @location(0) position: vec2f,
    @location(1) tex_coords: vec2f,
    @location(2) color: vec4f,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4f(position, 0.0, 1.0);
    out.tex_coords = tex_coords;
    out.color = color;
    return out;
}

/// 检查点是否在圆角区域内
fn is_in_rounded_corner(pos: vec2f, corner: vec2f, radius: f32) -> bool {
    if (radius <= 0.0) {
        return true;
    }
    let d = distance(pos, corner);
    return d <= radius;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    let tex_color = textureSample(u_texture, u_sampler, in.tex_coords);
    // 使用纹理的红通道作为 alpha 遮罩
    let alpha = tex_color.r;

    // 应用圆角裁剪
    let radius = u_uniforms.corner_radius;
    if (radius > 0.0) {
        let screen_w = u_uniforms.screen_width;
        let screen_h = u_uniforms.screen_height;

        // 将裁剪空间坐标转换为像素坐标
        let pixel_x = (in.position.x + 1.0) * 0.5 * screen_w;
        let pixel_y = (1.0 - in.position.y) * 0.5 * screen_h;

        // 左下角
        if (pixel_x < radius && pixel_y < radius) {
            if (!is_in_rounded_corner(vec2f(pixel_x, pixel_y), vec2f(radius, radius), radius)) {
                discard;
            }
        }
        // 右下角
        if (pixel_x > screen_w - radius && pixel_y < radius) {
            if (!is_in_rounded_corner(vec2f(pixel_x, pixel_y), vec2f(screen_w - radius, radius), radius)) {
                discard;
            }
        }
        // 左上角
        if (pixel_x < radius && pixel_y > screen_h - radius) {
            if (!is_in_rounded_corner(vec2f(pixel_x, pixel_y), vec2f(radius, screen_h - radius), radius)) {
                discard;
            }
        }
        // 右上角
        if (pixel_x > screen_w - radius && pixel_y > screen_h - radius) {
            if (!is_in_rounded_corner(vec2f(pixel_x, pixel_y), vec2f(screen_w - radius, screen_h - radius), radius)) {
                discard;
            }
        }
    }

    return vec4f(in.color.rgb, in.color.a * alpha);
}
