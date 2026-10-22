//! GPU 顶点类型:`BgVertex`(背景色 quad)/`TextVertex`(字形 quad)/
//! `BgImageVertex`(整屏背景图 quad)/`ImageQuadVertex`(Sixel/chunked-graphics
//! /inline-image 内联图 quad) 以及 uniform 结构 `ScreenUniforms`。
//!
//! 每个顶点结构都通过 `wgpu::VertexBufferLayout` 暴露对应的属性描述符,
//! 供 `pipeline.rs` 在创建渲染管线时绑定。布局/字段与拆分前完全一致。

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BgVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

impl BgVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = [
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x2,
        },
        wgpu::VertexAttribute {
            offset: std::mem::size_of::<[f32; 2]>() as u64,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x4,
        },
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TextVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
    pub atlas_selector: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

impl TextVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 6] = [
        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
        wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
        wgpu::VertexAttribute { offset: 16, shader_location: 2, format: wgpu::VertexFormat::Float32x4 },
        wgpu::VertexAttribute { offset: 32, shader_location: 3, format: wgpu::VertexFormat::Float32 },
        wgpu::VertexAttribute { offset: 36, shader_location: 4, format: wgpu::VertexFormat::Float32 },
        wgpu::VertexAttribute { offset: 40, shader_location: 5, format: wgpu::VertexFormat::Float32 },
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

// GAP-54:背景图 pass 的顶点(position + UV)。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BgImageVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

impl BgImageVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = [
        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
        wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

// T-core-graphics v3:image quad pass 的顶点。
// 布局与 `BgImageVertex`(`position: vec2f, uv: vec2f`)完全一致,
// 但独立成类型使 image quad 管线可以独立演进
//(例如未来加入逐图像着色、alpha 或 z)。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ImageQuadVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

impl ImageQuadVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = [
        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
        wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// 屏幕级 uniform。**四个 shader 共用同一份缓冲区**(背景色、单元
/// 文本、背景图、图像四边形),因此 WGSL 里的 `ScreenUniforms` 布局
/// 必须与本结构体逐字段一致。
///
/// 回归背景:四个 WGSL 里的 `ScreenUniforms` 各写各的
/// (16 / 16 / 32 / 32 字节),Rust 侧却是 28 字节。缓冲区比 shader
/// 期望的小 → wgpu 校验失败 → **应用直接 abort**:
/// `Buffer is bound with size 28 where the shader expects 32`。
/// 也就是说「背景图」和「Sixel / 内联图像」两个功能一开就崩。
///
/// 现在统一为 32 字节(8 × f32/u32),与 `bg_image.wgsl` 的布局一致,
/// 其余三个 shader 也改成同样的定义。字段顺序即内存顺序。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ScreenUniforms {
    pub corner_radius: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    /// GAP-54:背景图不透明度,取值 [0,1];0 = 无图像。
    pub bg_opacity: f32,
    /// GAP-54:0 = 居中,1 = 平铺,2 = 拉伸。用 u32 以保持 4 字节对齐。
    pub bg_mode: u32,
    /// GAP-54:图像原生像素尺寸(用于居中计算)。
    pub bg_img_width: f32,
    pub bg_img_height: f32,
    /// 尾部填充:把结构体补到 32 字节,与 WGSL 侧的
    /// `_padding: f32` 对齐。**不要删** —— 删掉会让缓冲区小于
    /// shader 的期望,直接触发 wgpu 校验失败并 abort 整个应用。
    pub _padding: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四个 shader 共用同一份 `ScreenUniforms` 缓冲区,任何一边改了
    /// 字段布局而另一边没跟上,wgpu 的校验就会失败 —— 而且是
    /// **abort 整个应用**级别(`Handling wgpu errors as fatal`)。
    ///
    /// 回归背景:WGSL 侧四个 shader 各自写了不同的布局
    /// (16 / 16 / 32 / 32 字节),Rust 侧是 28 字节,于是「背景图」和
    /// 「Sixel / 内联图像」一开就崩:
    /// `Buffer is bound with size 28 where the shader expects 32`。
    ///
    /// 这条测试直接解析 WGSL 源码,把每个 shader 声明的
    /// `ScreenUniforms` 字段序列与 Rust 结构体对齐检查:
    /// 字段名、顺序、字节大小三者必须全部一致。
    #[test]
    fn screen_uniforms_layout_matches_every_shader() {
        const SHADERS: &[&str] = &[
            "bg_color.wgsl",
            "cell_text.wgsl",
            "bg_image.wgsl",
            "image_quad.wgsl",
        ];

        let rust_fields: &[(&str, usize)] = &[
            ("corner_radius", 4),
            ("screen_width", 4),
            ("screen_height", 4),
            ("bg_opacity", 4),
            ("bg_mode", 4),
            ("bg_img_width", 4),
            ("bg_img_height", 4),
            ("_padding", 4),
        ];
        let rust_size = std::mem::size_of::<ScreenUniforms>();
        assert_eq!(
            rust_size,
            rust_fields.iter().map(|(_, n)| n).sum::<usize>(),
            "Rust 侧的字段大小之和与结构体大小不一致(存在隐式填充?)"
        );

        for shader in SHADERS {
            // `CARGO_MANIFEST_DIR` = crates/pureterm,shader 在同级 crate 里。
            let path = format!(
                "{}/../pureterm-alacritty-runtime/src/alacritty/renderer/shaders/{shader}",
                env!("CARGO_MANIFEST_DIR")
            );
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("读取 {path} 失败: {e}"));
            let body = extract_struct(&source, "ScreenUniforms")
                .unwrap_or_else(|| panic!("{shader} 里没有 struct ScreenUniforms"));
            let wgsl_fields: Vec<(String, usize)> = body
                .lines()
                .filter_map(|line| {
                    let line = line.split("//").next().unwrap_or("").trim();
                    if line.is_empty() {
                        return None;
                    }
                    let (name, ty) = line.split_once(':')?;
                    let name = name.trim().to_string();
                    let ty = ty.trim().trim_end_matches(',').trim();
                    // WGSL uniform 布局里 f32 / u32 / i32 各占 4 字节,
                    // vec2/3/4 是 8/16/16 —— 这里只用到标量。
                    let size = match ty {
                        "f32" | "u32" | "i32" => 4,
                        other => panic!("{shader}: 未预期的标量类型 {other}"),
                    };
                    Some((name, size))
                })
                .collect();

            assert_eq!(
                wgsl_fields.len(),
                rust_fields.len(),
                "{shader}: ScreenUniforms 字段数({})与 Rust 侧({})不一致",
                wgsl_fields.len(),
                rust_fields.len()
            );

            for (i, (wgsl_name, wgsl_size)) in wgsl_fields.iter().enumerate() {
                let (rust_name, rust_field_size) = rust_fields[i];
                assert_eq!(
                    *wgsl_size, rust_field_size,
                    "{shader}: 字段 {wgsl_name} 的大小与 Rust 侧不一致"
                );
                // WGSL 用 `opacity` / `mode` / `img_width` / `img_height`,
                // Rust 用 `bg_opacity` / `bg_mode` / `bg_img_*`。
                let equivalent = wgsl_name == rust_name
                    || (wgsl_name == "opacity" && rust_name == "bg_opacity")
                    || (wgsl_name == "mode" && rust_name == "bg_mode")
                    || (wgsl_name == "img_width" && rust_name == "bg_img_width")
                    || (wgsl_name == "img_height" && rust_name == "bg_img_height");
                assert!(
                    equivalent,
                    "{shader}: 第 {i} 个字段是 `{wgsl_name}`,Rust 侧对应的是 `{rust_name}` —— 顺序/命名不一致"
                );
            }

            let wgsl_size: usize = wgsl_fields.iter().map(|(_, n)| n).sum();
            assert_eq!(
                wgsl_size, rust_size,
                "{shader}: ScreenUniforms 字节数({wgsl_size})与 Rust 侧({rust_size})不一致 —— \
                 缓冲区小于 shader 期望会让 wgpu 校验失败并 abort 整个应用"
            );
        }
    }

    /// 从 WGSL 源码里取出 `struct <name> { ... }` 的花括号内容。
    fn extract_struct(source: &str, name: &str) -> Option<String> {
        let start = source.find(&format!("struct {name}"))?;
        let open = source[start..].find('{')? + start;
        let mut depth = 0usize;
        for (offset, ch) in source[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(source[open + 1..open + offset].to_string());
                    }
                },
                _ => {},
            }
        }
        None
    }
}
