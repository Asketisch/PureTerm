struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) tex_coords: vec2f,
    @location(1) color: vec4f,
}

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

@group(0) @binding(0)
var u_texture: texture_2d<f32>;

@group(0) @binding(1)
var u_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    let tex_color = textureSample(u_texture, u_sampler, in.tex_coords);
    // Use texture as alpha mask for text, solid color for backgrounds
    let alpha = tex_color.r;
    return vec4f(in.color.rgb, in.color.a * alpha);
}
