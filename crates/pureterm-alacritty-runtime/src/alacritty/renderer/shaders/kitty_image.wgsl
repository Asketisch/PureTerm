// Kitty image pass — STUB (no drawing).
// Minimal shader that compiles and renders nothing visible.

struct ScreenUniforms {
    corner_radius: f32,
    screen_width: f32,
    screen_height: f32,
    _padding: f32,
}

@group(0) @binding(0)
var u_texture: texture_2d<f32>;

@group(0) @binding(1)
var u_sampler: sampler;

@group(1) @binding(0)
var<uniform> u_screen: ScreenUniforms;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4f {
    // Off-screen degenerate triangle.
    return vec4f(2.0, 2.0, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4f {
    // Fully transparent — no visible output.
    return vec4f(0.0, 0.0, 0.0, 0.0);
}
