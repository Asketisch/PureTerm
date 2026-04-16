//! Math utilities for GPU rendering.
//!
//! Provides projection matrix and coordinate conversion helpers.

/// Create an orthographic projection matrix for 2D terminal rendering.
/// Maps pixel coordinates [0, width] x [0, height] to NDC [-1, 1].
pub fn orthographic(width: f32, height: f32) -> glam::Mat4 {
    glam::Mat4::orthographic_lh(0.0, width, height, 0.0, -1.0, 1.0)
}

/// Convert pixel coordinates to normalized device coordinates (NDC) [-1, 1].
#[inline]
pub fn pixel_to_ndc(x: f32, y: f32, screen_w: f32, screen_h: f32) -> [f32; 2] {
    [
        (x / screen_w) * 2.0 - 1.0,
        1.0 - (y / screen_h) * 2.0,
    ]
}
