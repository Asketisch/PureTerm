//! Kitty 图片渲染通道 (Stub)。
//!
//! 管线已创建但 update() 和 render() 为空操作。
//! 待实现：使用 RGBA 纹理采样渲染内联图片四边形（Kitty 协议）。

use wgpu::{Device, Queue, SurfaceConfiguration};

use super::{create_linear_sampler, create_simple_pipeline, create_texture_sampler_bgl, create_uniform_bgl, PassData, RenderPass};

/// Kitty 图片渲染通道 (Stub)。
pub struct KittyImagePass {
    #[allow(dead_code)]
    pipeline: wgpu::RenderPipeline,
    #[allow(dead_code)]
    sampler: wgpu::Sampler,
}

impl KittyImagePass {
    /// 创建新的 Kitty 图片通道 (Stub)。
    pub fn new(
        device: &Device,
        _queue: &Queue,
        _config: &SurfaceConfiguration,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let uniform_bgl = create_uniform_bgl(
            device,
            "KittyImage Uniform BGL",
            wgpu::ShaderStages::FRAGMENT,
        );
        let tex_sampler_bgl = create_texture_sampler_bgl(
            device,
            "KittyImage Texture Sampler BGL",
            wgpu::ShaderStages::FRAGMENT,
        );

        let _pipeline = create_simple_pipeline(
            device,
            "KittyImage Pipeline",
            include_str!("../shaders/kitty_image.wgsl"),
            wgpu::VertexBufferLayout {
                array_stride: 0,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[],
            },
            &[&tex_sampler_bgl, &uniform_bgl],
            surface_format,
        );

        let sampler = create_linear_sampler(device, "KittyImage Sampler");

        Self {
            pipeline: _pipeline,
            sampler,
        }
    }
}

impl RenderPass for KittyImagePass {
    fn label(&self) -> &'static str {
        "KittyImage"
    }

    fn update_buffers(&mut self, _device: &Device, _queue: &Queue, _data: &PassData) {
        // Stub: 空操作
    }

    fn render<'a>(&'a self, _render_pass: &mut wgpu::RenderPass<'a>) {
        // Stub: 空操作
    }

    fn resize(&mut self, _device: &Device, _config: &SurfaceConfiguration) {
        // Stub: 空操作
    }
}
