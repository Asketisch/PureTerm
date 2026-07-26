// T-core-graphics v3: image quad pass.
//
// Samples the image atlas (a single Rgba8UnormSrgb texture that packs all
// visible Sixel / chunked-graphics / inline images side by side) at per-vertex
// atlas UV coords. Each image is rendered as a screen-space quad whose size
// comes from the host CPU (col, row, cols, rows * cell_w, cell_h).
//
// The atlas lives at pipeline scope so the texture survives across frames;
// only `queue.write_texture` calls on dirty sub-rects re-upload new content.
//
// Bind group layout mirrors `bg_image.wgsl` for consistency:
//   @group(0) @binding(0) → ScreenUniforms (corner_radius + screen size)
//   @group(1) @binding(0) → texture_2d<f32> image atlas
//   @group(1) @binding(1) → sampler (linear filtering for image data)

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

struct ScreenUniforms {
    corner_radius: f32,
    screen_width: f32,
    screen_height: f32,
    _padding0: f32,
    _padding1: f32,
    _padding2: f32,
    _padding3: f32,
    _padding4: f32,
}

@group(0) @binding(0)
var<uniform> u_screen: ScreenUniforms;

@group(1) @binding(0)
var r_image_atlas: texture_2d<f32>;
@group(1) @binding(1)
var r_atlas_sampler: sampler;

@vertex
fn vs_main(
    @location(0) position: vec2f,   // logical pixels; vertex shader maps to NDC
    @location(1) uv: vec2f,         // atlas UV [0..1]
) -> VertexOutput {
    var out: VertexOutput;
    let ndc_x = position.x / u_screen.screen_width * 2.0 - 1.0;
    let ndc_y = 1.0 - position.y / u_screen.screen_height * 2.0;
    out.position = vec4f(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn is_in_rounded_corner(pos: vec2f, corner: vec2f, radius: f32) -> bool {
    if (radius <= 0.0) {
        return true;
    }
    return distance(pos, corner) <= radius;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    // Reuse the same rounded-corner clip as cell_text.wgsl / bg_color.wgsl so
    // images respect the global window corner radius.
    let radius = u_screen.corner_radius;
    if (radius > 0.0) {
        let sw = u_screen.screen_width;
        let sh = u_screen.screen_height;
        let x = in.position.x;
        let y = in.position.y;
        if (x < radius && y < radius && !is_in_rounded_corner(vec2f(x, y), vec2f(radius, radius), radius)) { discard; }
        if (x > sw - radius && y < radius && !is_in_rounded_corner(vec2f(x, y), vec2f(sw - radius, radius), radius)) { discard; }
        if (x < radius && y > sh - radius && !is_in_rounded_corner(vec2f(x, y), vec2f(radius, sh - radius), radius)) { discard; }
        if (x > sw - radius && y > sh - radius && !is_in_rounded_corner(vec2f(x, y), vec2f(sw - radius, sh - radius), radius)) { discard; }
    }
    return textureSample(r_image_atlas, r_atlas_sampler, in.uv);
}