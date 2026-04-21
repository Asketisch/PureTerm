struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) tex_coords: vec2f,
    @location(1) color: vec4f,
    @location(2) shape_params: vec4f,
}

/// 终端渲染 Uniforms
struct TerminalUniforms {
    corner_radius: f32,   // 屏幕边缘圆角半径（像素）
    screen_width: f32,    // 屏幕宽度
    screen_height: f32,   // 屏幕高度
}

@group(0) @binding(0)
var u_texture: texture_2d<f32>;

@group(0) @binding(1)
var u_sampler: sampler;

@group(0) @binding(2)
var<uniform> u_uniforms: TerminalUniforms;

/// 圆角矩形 SDF（Signed Distance Field）
/// p: 相对于矩形中心的点坐标
/// b: 矩形半尺寸 - corner_radius
/// r: 圆角半径
fn sd_rounded_box(p: vec2f, b: vec2f, r: f32) -> f32 {
    let q = abs(p) - b + vec2f(r);
    return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@vertex
fn vs_main(
    @location(0) position: vec2f,
    @location(1) tex_coords: vec2f,
    @location(2) color: vec4f,
    @location(3) shape_params: vec4f,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4f(position, 0.0, 1.0);
    out.tex_coords = tex_coords;
    out.color = color;
    out.shape_params = shape_params;
    return out;
}

/// 检查点是否在圆角区域内（屏幕边缘裁剪）
fn is_in_rounded_corner(pos: vec2f, corner: vec2f, radius: f32) -> bool {
    if (radius <= 0.0) {
        return true;
    }
    let d = distance(pos, corner);
    return d <= radius;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    let shape_type = in.shape_params.w;

    // shape_type == 0: 字形 / 纯色矩形（原有行为）
    if (shape_type < 0.5) {
        let tex_color = textureSample(u_texture, u_sampler, in.tex_coords);
        // 使用纹理的红通道作为 alpha 遮罩
        let alpha = tex_color.r;

        // 应用屏幕边缘圆角裁剪
        let radius = u_uniforms.corner_radius;
        if (radius > 0.0) {
            let screen_w = u_uniforms.screen_width;
            let screen_h = u_uniforms.screen_height;

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

        return vec4f(in.color.rgb, in.color.a * alpha);
    }

    // shape_type == 1: 圆角矩形 SDF
    if (shape_type < 1.5) {
        // tex_coords 是 0-1 范围的局部 UV
        let rect_size = in.shape_params.xy;
        let corner_radius = in.shape_params.z;

        // 将 UV 转换为相对于矩形中心的局部坐标（像素）
        let local_pos = (in.tex_coords - vec2f(0.5)) * rect_size;
        let half_size = rect_size * 0.5;

        // 计算 SDF
        let d = sd_rounded_box(local_pos, half_size, corner_radius);

        // 使用 fwidth 实现像素级抗锯齿
        let aa = fwidth(d);
        let alpha = 1.0 - smoothstep(-aa, aa, d);

        return vec4f(in.color.rgb, in.color.a * alpha);
    }

    // shape_type == 2: 圆形 SDF
    {
        // tex_coords 是 -1 到 1 范围的局部坐标（圆心为原点）
        let local_pos = in.tex_coords;
        let d = length(local_pos) - 1.0;

        // 使用 fwidth 实现像素级抗锯齿
        let aa = fwidth(d);
        let alpha = 1.0 - smoothstep(-aa, aa, d);

        return vec4f(in.color.rgb, in.color.a * alpha);
    }
}
