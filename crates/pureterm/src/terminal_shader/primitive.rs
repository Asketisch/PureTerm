//! iced_wgpu `Primitive` 实现:`TerminalPrimitive` 是每帧由
//! `TerminalProgram::draw` 构建的一次性 GPU draw-call 集合,持有 bg vertices、
//! text vertices、atlas 脏数据与 image quad 共享句柄。
//!
//! 由于 Rust 不允许把同一 trait impl 拆到多个文件,这里保留
//! `impl Primitive for TerminalPrimitive` 作为外壳,把 `prepare()` 的实际逻辑
//! 委托给 [`super::primitive_prepare`] 与 [`super::primitive_quad_pass`] 中的
//! 自由函数,从而把单文件行数控制在警戒线以下。

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use iced::Rectangle;
use iced_wgpu::graphics::Viewport;
use iced_wgpu::primitive::Primitive;

use crate::background_image::{BackgroundMode, DecodedBackground};
use pureterm_alacritty_runtime::renderer::glyph_atlas::AtlasDirty;

use super::helpers::f32_to_bits;
use super::image_quad::{ImageQuadDraw, ImageQuadState};
use super::pipeline::{GpuResources, TerminalPipeline};
use super::primitive_prepare::{
    prepare_atlas_textures, prepare_bg_color_pass, prepare_bg_image_pass, prepare_cell_text_pass,
};
use super::primitive_quad_pass::{
    drain_decode_channel, prepare_image_atlas_texture, prepare_image_quad_buffers,
};
use super::vertices::{BgVertex, ScreenUniforms, TextVertex};

pub struct TerminalPrimitive {
    pub(in crate::terminal_shader) bg_vertices: Vec<BgVertex>,
    pub(in crate::terminal_shader) bg_indices: Vec<u32>,
    /// bg 通道里「全屏底色 quad」的索引条数(= 6)。其余索引属于
    /// 单元格底色 / 选区 / 触发器高亮。绘制时按 base / 其余两段
    /// 分开画,中间插入背景图 —— 否则不透明的全屏底色会把背景图盖住。
    pub(in crate::terminal_shader) bg_base_index_count: u32,
    pub(in crate::terminal_shader) text_vertices: Vec<TextVertex>,
    pub(in crate::terminal_shader) text_indices: Vec<u32>,
    // 部分上传的脏 atlas(Fix 2):保存脏矩形列表 + 一个 Arc 共享的完整
    // atlas 缓冲区。渲染时遍历区域,每个区域发一次 `queue.write_texture`,避免
    // 之前每个脏帧对整个 4 MiB / 16 MiB atlas 的 `Vec::clone()`。`Arc` 让主线程
    // 和渲染线程共享同一分配。
    pub(in crate::terminal_shader) grayscale_atlas: Option<AtlasDirty>,
    pub(in crate::terminal_shader) color_atlas: Option<AtlasDirty>,
    /// 每帧 GPU 缓冲区 —— 每个 Primitive 实例拥有自己的缓冲区,
    /// 避免分屏时共享 Pipeline 相互覆盖的 bug。
    pub(in crate::terminal_shader) gpu: Mutex<GpuResources>,
    /// 共享缩放因子:`prepare()` 每帧用实际视口缩放因子更新它;
    /// `draw()` 读取它来检测变化。
    pub(in crate::terminal_shader) viewport_scale_factor: Arc<AtomicU32>,
    /// GAP-54:缓存的背景图片、模式、不透明度以及签名
    /// (已加载字节的 FNV-1a)。禁用或无文件时为 `None`。
    pub(in crate::terminal_shader) background_image: Option<DecodedBackground>,
    pub(in crate::terminal_shader) background_mode: BackgroundMode,
    pub(in crate::terminal_shader) background_opacity: f32,
    pub(in crate::terminal_shader) background_signature: u64,

    // T-core-graphics v3:image quad 状态。从 `TerminalProgram` 上的共享 `Arc`
    // 克隆而来,使渲染线程可以:
    //   1. 把 `decode_rx` 排入 `image_quad_state`(将解码后的 RGBA
    //      像素推入 CPU 侧 `ImageAtlas`)。
    //   2. 取出 `image_quad_draws`(UI 线程填充)为 GPU atlas 构建顶点/索引
    //      缓冲区。
    //
    // `Mutex` 持有时间很短(`try_recv()` / insert / clone-drain),绝不在
    // `.await` 期间持有 —— 它只是让 `Primitive::prepare(&self, ...)`(仅取
    // `&self`)满足 `Sync`。
    pub(in crate::terminal_shader) image_quad_state: Arc<Mutex<ImageQuadState>>,
    pub(in crate::terminal_shader) image_quad_draws: Arc<Mutex<Vec<ImageQuadDraw>>>,
    pub(in crate::terminal_shader) decode_rx:
        Arc<Mutex<std::sync::mpsc::Receiver<crate::image_decode_worker::DecodedImage>>>,
    // 逻辑像素下的单元格尺寸,构造时从 `glyph_state` 缓存。
    // image quad 通道用 `col * cell_w` 和 `row * cell_h` 计算屏幕空间四边形。
    // 缓存避免在 `prepare()`(运行于渲染线程)内加 glyph 状态锁。
    pub(in crate::terminal_shader) cell_w: f32,
    pub(in crate::terminal_shader) cell_h: f32,
}

impl fmt::Debug for TerminalPrimitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalPrimitive")
            .field("bg_verts", &self.bg_vertices.len())
            .field("text_verts", &self.text_vertices.len())
            .field("gpu", &*self.gpu.lock().unwrap())
            .finish()
    }
}

impl TerminalPrimitive {
    /// GAP-54:安装新的背景图片(解码后的 RGBA8)。传 `None` 清除。
    /// 签名是调用方提供的源字节哈希 —— 用于避免跨帧重复上传相同内容。
    /// 模式和不透明度在绘制时应用到纹理;每次调用都要传入。
    #[allow(dead_code)]
    pub fn set_background_image(
        &mut self,
        image: Option<DecodedBackground>,
        mode: BackgroundMode,
        opacity: f32,
        signature: u64,
    ) {
        self.background_image = image;
        self.background_mode = mode;
        self.background_opacity = opacity.clamp(0.0, 1.0);
        self.background_signature = signature;
    }
}

impl Primitive for TerminalPrimitive {
    type Pipeline = TerminalPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        // 发布实际视口缩放因子,使 `draw()`(下一帧)可以在需要时重配字形 atlas。
        let vp_scale = viewport.scale_factor();
        self.viewport_scale_factor.store(
            f32_to_bits(vp_scale),
            Ordering::Relaxed,
        );

        // 保持 uniform 为逻辑坐标 —— iced_wgpu 把 GPU 视口设为 bounds × scale_factor,
        // 因此顶点着色器的 NDC 换算必须与顶点位置使用同一套逻辑坐标系。
        let uniforms = ScreenUniforms {
            corner_radius: 0.0,
            screen_width: bounds.width,
            screen_height: bounds.height,
            bg_opacity: 0.0,
            bg_mode: 0,
            bg_img_width: 0.0,
            bg_img_height: 0.0,
            _padding: 0.0,
        };

        let mut gpu = self.gpu.lock().unwrap();

        // --- 背景色通道 ---
        prepare_bg_color_pass(self, pipeline, device, bounds, &mut gpu, &uniforms);

        // --- GAP-54:背景图片通道 ---
        prepare_bg_image_pass(self, pipeline, device, queue, bounds, &mut gpu);

        // --- Atlas 纹理(常驻) ---
        prepare_atlas_textures(self, pipeline, device, queue);

        // --- T-core-graphics v3:image quad 通道 ---
        // 步骤 A:排空解码通道。
        drain_decode_channel(self);
        // 步骤 B:把脏 atlas 区域上传到 GPU 纹理。
        prepare_image_atlas_texture(self, pipeline, device, queue);
        // 步骤 C:根据 `image_quad_draws` 构建每帧顶点/索引缓冲区。
        prepare_image_quad_buffers(self, pipeline, device, &mut gpu, &uniforms);

        // --- 单元格文本通道 ---
        prepare_cell_text_pass(self, pipeline, device, &mut gpu, &uniforms);

        drop(gpu); // 在后续可能使用 self 之前释放锁
    }

    fn draw(
        &self,
        pipeline: &Self::Pipeline,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        let gpu = self.gpu.lock().unwrap();

        // 绘制顺序(从下到上):
        //   1. 全屏底色(bg 通道的第 0..base 段)
        //   2. 背景图(可选;带 alpha,叠在底色之上)
        //   3. 单元格底色 / 选区 / 触发器高亮(bg 通道的 base..count 段)
        //   4. 图像四边形(Sixel / 内联图像)
        //   5. 文本
        //
        // 回归背景:早先把背景图排在最前,随后被**不透明**的全屏底色
        // 整个盖住 —— 背景图功能等于没有。现在把 bg 通道拆成两段,
        // 中间插入背景图。
        let bg_color_ready = gpu.bg_index_count > 0
            && gpu.bg_vertex_buffer.is_some()
            && gpu.bg_index_buffer.is_some()
            && gpu.bg_bind_group.is_some();

        // 1) 全屏底色
        if bg_color_ready {
            let (vb, ib, bg) = (
                gpu.bg_vertex_buffer.as_ref().unwrap(),
                gpu.bg_index_buffer.as_ref().unwrap(),
                gpu.bg_bind_group.as_ref().unwrap(),
            );
            // 夹一下索引区间:wgpu 的校验错误默认是**致命**的
            // (会 abort 整个应用),越界的 draw 区间绝不能发出去。
            let base = gpu.bg_base_index_count.min(gpu.bg_index_count);
            render_pass.set_pipeline(&pipeline.bg_color_pipeline);
            render_pass.set_bind_group(0, bg, &[]);
            render_pass.set_vertex_buffer(0, vb.slice(..));
            render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..base, 0, 0..1);
        }

        // 2) 背景图
        if gpu.bg_image_index_count > 0 {
            if let (Some(vb), Some(ib), Some(img_bg), Some(uniform_bg)) = (
                &gpu.bg_image_vertex_buffer,
                &gpu.bg_image_index_buffer,
                &gpu.bg_image_bind_group,
                &gpu.bg_image_uniform_bind_group,
            ) {
                // 背景图管线按需创建(首次绘制时才编译 shader)。
                render_pass.set_pipeline(
                    pipeline.bg_image_pipeline.get(&pipeline.device, pipeline.format),
                );
                render_pass.set_bind_group(0, uniform_bg, &[]);
                render_pass.set_bind_group(1, img_bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.bg_image_index_count, 0, 0..1);
            }
        }

        // 3) 单元格底色 / 选区 / 触发器高亮
        if bg_color_ready && gpu.bg_index_count > gpu.bg_base_index_count {
            let (vb, ib, bg) = (
                gpu.bg_vertex_buffer.as_ref().unwrap(),
                gpu.bg_index_buffer.as_ref().unwrap(),
                gpu.bg_bind_group.as_ref().unwrap(),
            );
            render_pass.set_pipeline(&pipeline.bg_color_pipeline);
            render_pass.set_bind_group(0, bg, &[]);
            render_pass.set_vertex_buffer(0, vb.slice(..));
            render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(
                gpu.bg_base_index_count..gpu.bg_index_count,
                0,
                0..1,
            );
        }

        // T-core-graphics v3:image quad 通道。绘制在 bg_color 与 cell_text 之间,
        // 使终端文本叠在图片之上(z 序规则:图片作为单元格背景,文本在上层 ——
        // 见 ExitPlanMode 计划 §0)。
        if gpu.image_quad_index_count > 0 {
            if let (Some(vb), Some(ib), Some(atlas_bg), Some(uniform_bg)) = (
                &gpu.image_quad_vertex_buffer,
                &gpu.image_quad_index_buffer,
                &pipeline.image_atlas_bind_group,
                &gpu.image_quad_uniform_bind_group,
            ) {
                // 图像四边形管线按需创建(首次绘制时才编译 shader)。
                render_pass.set_pipeline(
                    pipeline.image_quad_pipeline.get(&pipeline.device, pipeline.format),
                );
                render_pass.set_bind_group(0, uniform_bg, &[]);
                render_pass.set_bind_group(1, atlas_bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.image_quad_index_count, 0, 0..1);
            }
        }

        // 绘制单元格文本通道
        if gpu.text_index_count > 0 {
            if let (Some(vb), Some(ib), Some(atlas_bg), Some(uniform_bg)) = (
                &gpu.text_vertex_buffer, &gpu.text_index_buffer,
                &pipeline.text_atlas_bind_group, &gpu.text_uniform_bind_group,
            ) {
                render_pass.set_pipeline(&pipeline.text_pipeline);
                render_pass.set_bind_group(0, atlas_bg, &[]);
                render_pass.set_bind_group(1, uniform_bg, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..gpu.text_index_count, 0, 0..1);
            }
        }

        true
    }
}
