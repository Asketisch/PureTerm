struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) tex_coords: vec2f,
    @location(1) color: vec4f,
    @location(2) atlas_selector: f32,
    @location(4) x_offset: f32,
    @location(5) y_offset: f32,
}

struct ScreenUniforms {
    corner_radius: f32,
    screen_width: f32,
    screen_height: f32,
    _padding: f32,
}

// Grayscale atlas (R8Unorm).
@group(0) @binding(0)
var u_texture_gray: texture_2d<f32>;

@group(0) @binding(1)
var u_sampler_gray: sampler;

// Color atlas (Rgba8UnormSrgb).
@group(0) @binding(2)
var u_texture_color: texture_2d<f32>;

@group(0) @binding(3)
var u_sampler_color: sampler;

@group(1) @binding(0)
var<uniform> u_screen: ScreenUniforms;

@vertex
fn vs_main(
    @location(0) position: vec2f,
    @location(1) tex_coords: vec2f,
    @location(2) color: vec4f,
    @location(3) atlas_selector: f32,
    @location(4) x_offset: f32,
    @location(5) y_offset: f32,
) -> VertexOutput {
    var out: VertexOutput;
    let ndc_x = position.x / u_screen.screen_width * 2.0 - 1.0;
    let ndc_y = 1.0 - position.y / u_screen.screen_height * 2.0;
    out.position = vec4f(ndc_x, ndc_y, 0.0, 1.0);
    out.tex_coords = tex_coords;
    out.color = color;
    out.atlas_selector = atlas_selector;
    out.x_offset = x_offset;
    out.y_offset = y_offset;
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

    var final_color: vec4f;
    if (in.atlas_selector > 0.5) {
        // Color glyph: sample RGBA from color atlas directly.
        // crossfont returns premultiplied alpha, compatible with ALPHA_BLENDING.
        final_color = textureSample(u_texture_color, u_sampler_color, in.tex_coords);
    } else {
        // Grayscale glyph: use R channel as alpha mask for vertex color.
        let gray = textureSample(u_texture_gray, u_sampler_gray, in.tex_coords);
        final_color = vec4f(in.color.rgb, in.color.a * gray.r);
    }

    return final_color;
}
