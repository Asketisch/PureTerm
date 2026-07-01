// GAP-54: background image pass — samples a 2D texture and applies
// Center / Tile / Stretch mode with corner-radius clipping + opacity.

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

struct ScreenUniforms {
    corner_radius: f32,
    screen_width: f32,
    screen_height: f32,
    opacity: f32,
    /// 0 = Center, 1 = Tile, 2 = Stretch
    mode: u32,
    img_width: f32,
    img_height: f32,
    _padding: f32,
}

@group(0) @binding(0)
var<uniform> u_screen: ScreenUniforms;

@group(1) @binding(0)
var r_tex: texture_2d<f32>;
@group(1) @binding(1)
var r_sampler: sampler;

@vertex
fn vs_main(
    @location(0) position: vec2f,
    @location(1) uv: vec2f,
) -> VertexOutput {
    var out: VertexOutput;
    let ndc_x = position.x / u_screen.screen_width * 2.0 - 1.0;
    let ndc_y = 1.0 - position.y / u_screen.screen_height * 2.0;
    out.position = vec4f(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn in_rounded_corner(p: vec2f, c: vec2f, r: f32) -> bool {
    if (r <= 0.0) {
        return true;
    }
    return distance(p, c) <= r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    // Round-corner clip (mirrors bg_color.wgsl).
    let radius = u_screen.corner_radius;
    if (radius > 0.0) {
        let sw = u_screen.screen_width;
        let sh = u_screen.screen_height;
        let x = in.position.x;
        let y = in.position.y;
        if (x < radius && y < radius && !in_rounded_corner(vec2f(x, y), vec2f(radius, radius), radius)) { discard; }
        if (x > sw - radius && y < radius && !in_rounded_corner(vec2f(x, y), vec2f(sw - radius, radius), radius)) { discard; }
        if (x < radius && y > sh - radius && !in_rounded_corner(vec2f(x, y), vec2f(radius, sh - radius), radius)) { discard; }
        if (x > sw - radius && y > sh - radius && !in_rounded_corner(vec2f(x, y), vec2f(sw - radius, sh - radius), radius)) { discard; }
    }

    var sample_uv = in.uv;
    let mode = u_screen.mode;
    if (mode == 0u) {
        // Center: keep uv as-is, transparent where out-of-image bounds.
        // For now we trust the host CPU to pass 0..1 only over the image rect;
        // a fullscreen quad with uv always 0..1 will sample the whole image
        // which is wrong. We solve this in CPU by drawing a sub-quad.
    } else if (mode == 2u) {
        // Stretch: uv already 0..1 over the full screen quad.
    }
    // mode == 1 (Tile): the wgsl sampler does not support wrap by default;
    // we rely on the host CPU to repeat-quad tile the image.

    let rgba = textureSample(r_tex, r_sampler, sample_uv);
    return vec4f(rgba.rgb, rgba.a * u_screen.opacity);
}
