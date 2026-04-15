# 08 - 渲染系统

## 功能概述

完整的 GPU 加速终端渲染管线：支持 Metal (macOS)、OpenGL (Linux)、WebGL (WASM) 三种后端。独立的渲染线程、字形图集、实例化渲染、自定义着色器。

## Ghostty 源码文件

### 顶层渲染文件 (~18)
| 文件 | 行数 | 功能 |
|------|------|------|
| `renderer/generic.zig` | ~2300 | **核心**：通用渲染逻辑（GraphicsAPI 泛型） |
| `renderer/Metal.zig` | ~500 | Metal 后端包装 |
| `renderer/OpenGL.zig` | ~400 | OpenGL 后端包装 |
| `renderer/WebGL.zig` | ~10 | WebGL 占位 |
| `renderer/Thread.zig` | ~400 | 渲染线程（独立事件循环） |
| `renderer/shadertoy.zig` | ~400 | ShaderToy 自定义着色器 |
| `renderer/cell.zig` | ~500 | 单元格 GPU 缓冲区管理 |
| `renderer/image.zig` | ~600 | GPU 图像管理（Kitty/覆盖层） |
| `renderer/link.zig` | ~200 | URL/链接匹配和高亮 |
| `renderer/cursor.zig` | ~150 | 光标样式确定逻辑 |
| `renderer/row.zig` | ~50 | 行级背景扩展启发式 |
| `renderer/size.zig` | ~200 | 大小/坐标/填充类型 |
| `renderer/State.zig` | ~80 | 共享渲染状态 |
| `renderer/message.zig` | ~100 | 渲染线程消息 |
| `renderer/Options.zig` | ~30 | 渲染器配置 |
| `renderer/Overlay.zig` | ~300 | CPU 绘制调试覆盖层 |
| `renderer/backend.zig` | ~30 | 后端枚举选择 |

### Metal 后端 (10)
| 文件 | 功能 |
|------|------|
| `renderer/metal/api.zig` | Metal API 类型定义 |
| `renderer/metal/Target.zig` | IOSurface 渲染目标 |
| `renderer/metal/Frame.zig` | 命令缓冲区管理 |
| `renderer/metal/Pipeline.zig` | 渲染管线 |
| `renderer/metal/RenderPass.zig` | 渲染通道 |
| `renderer/metal/buffer.zig` | MTLBuffer 包装 |
| `renderer/metal/Texture.zig` | MTLTexture 包装 |
| `renderer/metal/Sampler.zig` | 采样器 |
| `renderer/metal/shaders.zig` | 着色器加载 |
| `renderer/metal/IOSurfaceLayer.zig` | IOSurface CALayer |

### OpenGL 后端 (8)
| 文件 | 功能 |
|------|------|
| `renderer/opengl/Target.zig` | FBO 渲染目标 |
| `renderer/opengl/Frame.zig` | 帧管理 |
| `renderer/opengl/Pipeline.zig` | GLSL 程序 |
| `renderer/opengl/RenderPass.zig` | 渲染通道 |
| `renderer/opengl/buffer.zig` | GL Buffer |
| `renderer/opengl/Texture.zig` | GL Texture |
| `renderer/opengl/Sampler.zig` | GL Sampler |
| `renderer/opengl/shaders.zig` | GLSL 着色器 |

## 核心架构

### 1. 渲染管线

```
IO/主线程 → Mailbox → 渲染线程
                         ↓
               updateFrame()  (更新终端状态到 GPU 缓冲区)
                         ↓
               drawFrame()    (多通道渲染)
                         ↓
               present()      (提交到显示)
```

### 2. 多通道渲染顺序

```
通道 1: 背景色或背景图像 (bg_color / bg_image pipeline)
通道 2: Kitty 图像 - 背景层之下
通道 3: 单元格背景 (cell_bg pipeline)
通道 4: Kitty 图像 - 文本层之下
通道 5: 单元格文本 (cell_text pipeline) - 字形+下划线+删除线+光标
通道 6: Kitty 图像 - 文本层之上
通道 7: 调试覆盖层
通道 8: 自定义后处理着色器 (ShaderToy 兼容)
```

### 3. GPU 缓冲区

```
CellBg = [4]u8                    - RGBA 背景色 (flat array)
CellText (32 bytes, extern):
  ├── glyph_pos: [2]u16          - 字形在图集中的位置
  ├── glyph_size: [2]u16         - 字形尺寸
  ├── bearings: [2]i16           - 字形偏移
  ├── grid_pos: [2]u16           - 网格位置
  ├── color: [4]u8               - RGBA 前景色
  ├── atlas: enum                - 灰度/彩色图集
  └── bools: packed              - no_min_contrast, is_cursor

Uniforms (GPU):
  ├── projection_matrix: [4]F32x4  - 正交投影
  ├── screen_size: [2]f32
  ├── cell_size: [2]u32
  ├── grid_size: [2]u32
  ├── grid_padding: [4]u32
  ├── min_contrast: f32
  ├── cursor_pos: [2]u32
  ├── cursor_color: [4]u8
  ├── bg_color: [4]u8
  └── bools: cursor_wide, use_display_p3, use_linear_blending
```

### 4. 交换链

```
Metal: 三缓冲 (3 个 FrameState) + 信号量同步
OpenGL: 单缓冲

FrameState:
  ├── uniforms: UniformBuffer
  ├── cells: CellTextBuffer
  ├── cells_bg: CellBgBuffer
  ├── grayscale: Texture (字体图集)
  ├── color: Texture (字体图集)
  ├── target: RenderTarget
  └── custom_shader_state: ?CustomShaderState
```

### 5. 字体图集

两个纹理图集（灰度 + 彩色 BGRA）：
- 使用矩形装箱算法（Jukka Jylanki）
- 字形按需加载到图集
- 渲染时通过 CellText 中的 atlas_x/y 引用

### 6. 自定义着色器

ShaderToy 兼容的后处理管线：
- 加载 GLSL → SPIR-V (glslang) → MSL/GLSL (SPIRV-Cross)
- 前后纹理 ping-pong 支持链式着色器
- Uniforms 包含：分辨率、时间、光标位置/颜色、调色板、焦点状态

### 7. 渲染线程

```
Thread:
  ├── 事件循环 (xev)
  ├── 邮箱 (BlockingQueue, 64)
  ├── 渲染定时器: 8ms (~120 FPS)
  ├── 光标闪烁定时器: 600ms
  ├── 状态标志: visible, focused, cursor_blink_visible
  └── QoS 级别: user_interactive > user_initiated > utility
```

## 纯 Rust 实现要点

1. **使用 wgpu 替代 Metal/OpenGL**：跨平台 GPU API
2. **渲染管线用 wgpu RenderPipeline**：5 个内置管线
3. **字体图集用 `guillotière` crate**：矩形装箱
4. **交换链用 wgpu Surface**：自带缓冲管理
5. **独立渲染线程**：`std::thread::spawn` + 消息通道
6. **自定义着色器**：WGSL 或 GLSL → SPIR-V → naga 转换

### wgpu 等价管线

```rust
// 5 个渲染管线
PIPELINE_BG_COLOR    // 全屏三角形，纯色背景
PIPELINE_BG_IMAGE    // 背景图像
PIPELINE_CELL_BG     // 单元格背景（SSBO）
PIPELINE_CELL_TEXT   // 字形实例化渲染
PIPELINE_IMAGE       // Kitty/覆盖层图像
```

### 关键 Rust 类型

```rust
pub struct Renderer {
    config: DerivedConfig,
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: PipelineSet,
    font_atlas_grayscale: TextureAtlas,
    font_atlas_color: TextureAtlas,
    cell_bg_buffer: GpuBuffer<CellBg>,
    cell_text_buffer: GpuBuffer<CellText>,
    uniform_buffer: GpuBuffer<Uniforms>,
    // ...
}
```
