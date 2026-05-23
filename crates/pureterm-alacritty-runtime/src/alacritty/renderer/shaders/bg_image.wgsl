struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) tex_coords: vec2f,
}

struct BgImageUniforms {
    corner_radius: f32,
    screen_width: f32,
    screen_height: f32,
    opacity: f32,
    image_width: f32,
    image_height: f32,
    fit: f32,
    position: f32,
}

@group(0) @binding(0)
var u_texture: texture_2d<f32>;

@group(0) @binding(1)
var u_sampler: sampler;

@group(1) @binding(0)
var<uniform> u_bg: BgImageUniforms;

// Compute position offset factor for 3x3 grid.
// Returns (x_factor, y_factor) where 0.0 = start, 0.5 = center, 1.0 = end.
fn position_factor(pos: f32) -> vec2f {
    // 0=Center(0.5,0.5), 1=TopLeft(0,0), 2=TopCenter(0.5,0), 3=TopRight(1,0),
    // 4=CenterLeft(0,0.5), 5=CenterRight(1,0.5),
    // 6=BottomLeft(0,1), 7=BottomCenter(0.5,1), 8=BottomRight(1,1)
    let x = select(
        select(
            select(0.5, 0.0, pos >= 0.5 && pos <= 1.5),
            1.0,
            pos >= 2.5 && pos <= 5.5,
        ),
        0.0,
        pos < 0.5,
    );
    // Simpler: map directly
    var xf: f32;
    var yf: f32;

    if (pos < 0.5) {
        // Center
        xf = 0.5; yf = 0.5;
    } else if (pos < 1.5) {
        // TopLeft
        xf = 0.0; yf = 0.0;
    } else if (pos < 2.5) {
        // TopCenter
        xf = 0.5; yf = 0.0;
    } else if (pos < 3.5) {
        // TopRight
        xf = 1.0; yf = 0.0;
    } else if (pos < 4.5) {
        // CenterLeft
        xf = 0.0; yf = 0.5;
    } else if (pos < 5.5) {
        // CenterRight
        xf = 1.0; yf = 0.5;
    } else if (pos < 6.5) {
        // BottomLeft
        xf = 0.0; yf = 1.0;
    } else if (pos < 7.5) {
        // BottomCenter
        xf = 0.5; yf = 1.0;
    } else {
        // BottomRight
        xf = 1.0; yf = 1.0;
    }

    return vec2f(xf, yf);
}

@vertex
fn vs_main(
    @location(0) position: vec2f,
    @location(1) tex_coords: vec2f,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4f(position, 0.0, 1.0);
    out.tex_coords = tex_coords;

    let screen_w = u_bg.screen_width;
    let screen_h = u_bg.screen_height;
    let img_w = u_bg.image_width;
    let img_h = u_bg.image_height;
    let fit = u_bg.fit;

    // Skip UV adjustment if no image dimensions
    if (img_w <= 0.0 || img_h <= 0.0) {
        return out;
    }

    let screen_aspect = screen_w / screen_h;
    let img_aspect = img_w / img_h;

    var scale_x: f32;
    var scale_y: f32;

    if (fit > 2.5) {
        // Stretch (3.0): identity
        scale_x = 1.0;
        scale_y = 1.0;
    } else if (fit > 1.5) {
        // Cover (2.0): image covers the screen
        if (img_aspect > screen_aspect) {
            scale_x = screen_aspect / img_aspect;
            scale_y = 1.0;
        } else {
            scale_x = 1.0;
            scale_y = img_aspect / screen_aspect;
        }
    } else if (fit > 0.5) {
        // Contain (1.0): image fits within the screen
        if (img_aspect > screen_aspect) {
            scale_x = 1.0;
            scale_y = screen_aspect / img_aspect;
        } else {
            scale_x = img_aspect / screen_aspect;
            scale_y = 1.0;
        }
    } else {
        // None (0.0): 1:1 pixel mapping
        scale_x = img_w / screen_w;
        scale_y = img_h / screen_h;
    }

    let pf = position_factor(u_bg.position);
    let offset_x = (1.0 - scale_x) * pf.x;
    let offset_y = (1.0 - scale_y) * pf.y;

    out.tex_coords = vec2f(
        tex_coords.x * scale_x + offset_x,
        tex_coords.y * scale_y + offset_y,
    );

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
    let tex_color = textureSample(u_texture, u_sampler, in.tex_coords);

    // Apply opacity
    var result = tex_color;
    result.a = result.a * u_bg.opacity;

    // Rounded corner discard
    let radius = u_bg.corner_radius;
    if (radius > 0.0) {
        let screen_w = u_bg.screen_width;
        let screen_h = u_bg.screen_height;

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

    return result;
}
