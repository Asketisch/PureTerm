//! ShaderToy-compatible custom shader support.
//!
//! Stores custom GLSL fragment shaders for post-processing effects.
//! Actual GLSL→WGSL compilation is deferred to when the shader is set on the render pass.
//!
//! The standard ShaderToy uniforms (iTime, iResolution, iMouse, etc.) are provided
//! via `ShaderToyUniforms` and can be passed to the post-process pipeline.

use std::time::{SystemTime, UNIX_EPOCH};

/// Standard ShaderToy uniforms (matching the ShaderToy convention).
/// These are passed as a uniform buffer to the post-process shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShaderToyUniforms {
    pub time: f32,
    pub resolution_x: f32,
    pub resolution_y: f32,
    pub frame: f32,
    pub time_delta: f32,
    pub frame_rate: f32,
    pub sample_rate: f32,
    pub date_year: f32,
    pub date_month: f32,
    pub date_day: f32,
    pub date_seconds: f32,
    /// iMouse: x, y, click_x, click_y (in pixels from bottom-left)
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_click_x: f32,
    pub mouse_click_y: f32,
}

impl ShaderToyUniforms {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            time: 0.0,
            resolution_x: width,
            resolution_y: height,
            frame: 0.0,
            time_delta: 0.0,
            frame_rate: 0.0,
            sample_rate: 44100.0,
            date_year: 0.0,
            date_month: 0.0,
            date_day: 0.0,
            date_seconds: 0.0,
            mouse_x: 0.0,
            mouse_y: 0.0,
            mouse_click_x: 0.0,
            mouse_click_y: 0.0,
        }
    }

    /// Update with current time and date.
    pub fn with_date(&mut self) {
        if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let secs = now.as_secs();
            let year = (secs / 31556952) + 1970; // approximate
            let remaining = secs % 31556952;
            let month = (remaining / 2629746) + 1; // approximate
            let day_remaining = remaining % 2629746;
            let day = (day_remaining / 86400) + 1;
            let sec_remaining = day_remaining % 86400;
            let hour = sec_remaining / 3600;
            let minute = (sec_remaining % 3600) / 60;
            let second = sec_remaining % 60;

            self.date_year = year as f32;
            self.date_month = month as f32;
            self.date_day = day as f32;
            self.date_seconds = hour as f32 * 3600.0
                + minute as f32 * 60.0
                + second as f32;
        }
    }
}

/// Input channel for a shader pass.
#[derive(Debug, Clone)]
pub enum ChannelInput {
    /// No input bound.
    None,
    /// Input from a buffer pass (buffer 0-3).
    Buffer(usize),
    /// Placeholder 1x1 texture.
    Placeholder,
}

/// A ShaderToy pass — stores the GLSL source for later compilation.
#[derive(Debug)]
pub struct ShaderPass {
    /// GLSL ES 3.0 fragment shader source (ShaderToy-compatible).
    glsl_source: String,
    /// Input channels bound to this pass.
    #[allow(dead_code)]
    channels: [ChannelInput; 4],
    /// Whether this pass renders to a buffer texture (for multi-pass).
    #[allow(dead_code)]
    render_to_buffer: bool,
}

/// The ShaderToy shader manager.
/// Stores GLSL sources; actual compilation deferred to render pass.
#[derive(Debug)]
pub struct ShaderToy {
    passes: Vec<ShaderPass>,
    /// Ping-pong buffer textures for multi-pass effects.
    buffer_textures: [Option<wgpu::TextureView>; 2],
    current_buffer: usize,
    start_time: std::time::Instant,
    frame_count: u32,
    enabled: bool,
    last_error: Option<String>,
    uniforms: ShaderToyUniforms,
    /// Mouse position for iMouse uniform.
    mouse: (f32, f32),
    /// Mouse click position.
    mouse_click: (f32, f32),
}

impl ShaderToy {
    pub fn new() -> Self {
        Self {
            passes: Vec::new(),
            buffer_textures: [None, None],
            current_buffer: 0,
            start_time: std::time::Instant::now(),
            frame_count: 0,
            enabled: false,
            last_error: None,
            uniforms: ShaderToyUniforms::new(0.0, 0.0),
            mouse: (0.0, 0.0),
            mouse_click: (0.0, 0.0),
        }
    }

    /// Load a ShaderToy-compatible GLSL fragment shader.
    /// Returns Ok(()) if the source looks valid (basic check), or an error message.
    ///
    /// The actual GLSL→WGSL conversion is done when the shader is applied to the pipeline.
    /// For now, we store the GLSL and let the post-process pass use a built-in WGSL shader.
    pub fn load(&mut self, glsl_source: &str) -> Result<(), String> {
        // Basic validation: shader must contain mainImage
        if !glsl_source.contains("mainImage") {
            return Err("Shader must define mainImage function".to_string());
        }
        if !glsl_source.contains("#version") {
            return Err("Shader must include #version directive".to_string());
        }

        self.passes.push(ShaderPass {
            glsl_source: glsl_source.to_string(),
            channels: [
                ChannelInput::None,
                ChannelInput::None,
                ChannelInput::None,
                ChannelInput::None,
            ],
            render_to_buffer: false,
        });
        self.enabled = true;
        self.last_error = None;
        Ok(())
    }

    /// Load the default vignette shader.
    pub fn load_default(&mut self) {
        let _ = self.load(DEFAULT_VIGNETTE_GLSL);
    }

    /// Create ping-pong textures for buffer passes.
    pub fn create_buffer_textures(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        for i in 0..2 {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(&format!("ShaderToy Buffer {}", i)),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.buffer_textures[i] = Some(tex.create_view(&wgpu::TextureViewDescriptor::default()));
        }
    }

    /// Update the uniforms for the current frame.
    pub fn update(&mut self, width: f32, height: f32, time_delta: f32) {
        self.uniforms.resolution_x = width;
        self.uniforms.resolution_y = height;
        self.uniforms.time = self.start_time.elapsed().as_secs_f32();
        self.uniforms.time_delta = time_delta;
        self.uniforms.frame = self.frame_count as f32;
        self.uniforms.frame_rate = if time_delta > 0.0 { 1.0 / time_delta } else { 0.0 };
        self.uniforms.with_date();
        self.uniforms.mouse_x = self.mouse.0;
        self.uniforms.mouse_y = self.mouse.1;
        self.uniforms.mouse_click_x = self.mouse_click.0;
        self.uniforms.mouse_click_y = self.mouse_click.1;
    }

    /// Advance to the next frame (updates ping-pong index and frame counter).
    pub fn next_frame(&mut self) {
        self.frame_count += 1;
        self.current_buffer = 1 - self.current_buffer;
    }

    /// Set the mouse position (pixels, origin at bottom-left).
    pub fn set_mouse(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
    }

    /// Set the mouse click position.
    pub fn set_mouse_click(&mut self, x: f32, y: f32) {
        self.mouse_click = (x, y);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn uniforms(&self) -> ShaderToyUniforms {
        self.uniforms
    }

    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }

    /// Get the GLSL source for a pass (for external GLSL→WGSL conversion).
    pub fn pass_glsl(&self, pass_idx: usize) -> Option<&str> {
        self.passes.get(pass_idx).map(|p| p.glsl_source.as_str())
    }
}

impl Default for ShaderToy {
    fn default() -> Self {
        Self::new()
    }
}

/// Default vignette post-process shader as GLSL ES 3.0.
pub const DEFAULT_VIGNETTE_GLSL: &str = r#"
#version 310 es
precision highp float;

in vec2 v_uv;
out vec4 fragColor;

uniform float u_time;
uniform vec2 u_resolution;
uniform float u_frame;
uniform float u_time_delta;
uniform float u_frame_rate;
uniform float u_sample_rate;
uniform vec4 u_date;
uniform vec4 u_mouse;

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / u_resolution;
    vec2 center = vec2(0.5);
    float dist = distance(uv, center);
    float vignette = 1.0 - smoothstep(0.4, 0.9, dist);
    fragColor = vec4(vec3(vignette), 1.0);
}
"#;
