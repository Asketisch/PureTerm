struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) tex_coords: vec2f,
}

struct PostProcessUniforms {
    time: f32,
    screen_width: f32,
    screen_height: f32,
    enabled: f32,
}

@group(0) @binding(0)
var u_scene: texture_2d<f32>;

@group(0) @binding(1)
var u_sampler: sampler;

@group(1) @binding(0)
var<uniform> u_uniforms: PostProcessUniforms;

@vertex
fn vs_main(
    @location(0) position: vec2f,
    @location(1) tex_coords: vec2f,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4f(position, 0.0, 1.0);
    out.tex_coords = tex_coords;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    let color = textureSample(u_scene, u_sampler, in.tex_coords);

    if (u_uniforms.enabled < 0.5) {
        return color;
    }

    // Simple vignette effect
    let uv = in.tex_coords;
    let center = vec2f(0.5, 0.5);
    let dist = distance(uv, center);
    let vignette = 1.0 - smoothstep(0.4, 0.9, dist);

    return vec4f(color.rgb * vignette, color.a);
}
