# pureterm-gui/src/render_pass — 多 pass GPU 渲染架构

将终端渲染拆分为 8 个独立 pass，每个 pass 负责一类视觉元素。所有 pass 实现 `RenderPass` trait，由 `TerminalRenderer` 按序调度。

## 文件结构

```
src/
├── mod.rs          # RenderPass trait + PassData + 工厂函数 (226 行)
├── bg_color.rs     # BgColorPass 单元格背景色 (150 行)
├── cell_text.rs    # CellTextPass 字形渲染 (184 行)
├── cursor.rs       # CursorPass 光标 + 闪烁 (203 行)
├── decorations.rs  # DecorationsPass 下划线/删除线/上划线 (148 行)
├── bg_image.rs     # BgImagePass 背景壁纸 (209 行)
├── kitty_image.rs  # KittyImagePass 内联图片 (184 行)
├── post_process.rs # PostProcessPass 后处理效果 (214 行)
└── debug_overlay.rs # DebugOverlayPass 调试文本 (164 行)
```

## 模块依赖

```
render_pass/ → wgpu（GPU API）
render_pass/ → pureterm-terminal（ScreenSnapshot、Cell）
render_pass/ → bytemuck（Pod/Zeroable 顶点数据）
```

## 渲染顺序

```
1. BgImagePass      — 全屏背景图片（可选壁纸）
2. BgColorPass      — 单元格背景色四边形
3. DecorationsPass  — 下划线/删除线/上划线装饰
4. CellTextPass     — 字形四边形（采样图集纹理）
5. KittyImagePass   — Kitty 协议内联图片
6. CursorPass       — 终端光标（支持闪烁）
7. PostProcessPass  — 全屏后处理（暗角、ShaderToy）
8. DebugOverlayPass — 调试文本覆盖
```

## 核心类型

### RenderPass trait（mod.rs）

所有 pass 的统一接口：

```rust
pub trait RenderPass {
    fn label() -> &'static str;
    fn update_buffers(&mut self, device: &Device, queue: &Queue, data: &PassData);
    fn render(&self, pass: &mut wgpu::RenderPass<'_>);
    fn resize(&mut self, device: &Device, new_size: (u32, u32));
}
```

### PassData（mod.rs）

每帧共享数据：

| 字段 | 类型 | 说明 |
|------|------|------|
| `screen_width` | `f32` | 屏幕宽度（像素） |
| `screen_height` | `f32` | 屏幕高度（像素） |
| `corner_radius` | `f32` | 圆角半径 |
| `texture_view` | `Option<TextureView>` | 共享纹理视图 |

### ScreenUniforms（mod.rs）

GPU uniform 结构：

```rust
#[repr(C)]
struct ScreenUniforms {
    corner_radius: f32,
    screen_width: f32,
    screen_height: f32,
}
```

### 工厂函数（mod.rs）

| 函数 | 说明 |
|------|------|
| `create_simple_pipeline()` | 创建标准 alpha blending 渲染管线 |
| `create_uniform_bgl()` | uniform bind group layout 工厂 |
| `create_texture_sampler_bgl()` | 纹理+采样器 bind group layout 工厂 |
| `create_nearest_sampler()` / `create_linear_sampler()` | 采样器工厂 |
| `max_vertex_buffer_size()` / `max_index_buffer_size()` | 预分配缓冲区大小（100K cells） |

## 各 Pass 详解

### BgColorPass（bg_color.rs）

单元格背景色渲染。

**顶点格式：** `BgVertex` — position [f32;2] + color [f32;4]

每个有非默认背景色的 Cell 生成一个四边形（4 顶点 + 6 索引）。通过 uniform buffer 传递圆角参数。

### CellTextPass（cell_text.rs）

字形渲染 pass，采样灰度字形图集纹理。

**顶点格式：** `TextVertex` — position + tex_coords + color

- `set_atlas_texture()` — 设置字形图集纹理绑定
- 使用 nearest-neighbor 采样保持字形锐利

### CursorPass（cursor.rs）

终端光标渲染，内置闪烁定时器。

**顶点格式：** `CursorVertex` — position + color

**Uniform：** `CursorUniforms` — corner_radius + screen_dims + blink_visible

| 方法 | 说明 |
|------|------|
| `update_blink()` | 每 500ms 切换闪烁状态 |
| `reset_blink()` | 用户输入时重置闪烁（使光标可见） |
| `is_blink_visible()` | 当前闪烁可见性 |

### DecorationsPass（decorations.rs）

文本装饰线渲染：下划线、删除线、上划线。

**顶点格式：** `DecoVertex` — position + color

为每个有装饰属性的 Cell 生成细矩形四边形。

### BgImagePass（bg_image.rs）

全屏背景壁纸渲染。

**顶点格式：** `BgImageVertex` — position + tex_coords

| 方法 | 说明 |
|------|------|
| `set_background(texture)` | 设置/清除背景纹理 |
| `set_uv_rect(rect)` | 根据缩放模式计算 UV 坐标 |
| `is_visible()` | 是否有背景图片 |

### KittyImagePass（kitty_image.rs）

Kitty 图形协议内联图片渲染。

**顶点格式：** `ImageVertex` — position + tex_coords + color tint

每个图片映射到 RGBA 纹理，使用 linear 采样。

### PostProcessPass（post_process.rs）

全屏后处理效果。

**顶点格式：** `PostProcessVertex` — position + tex_coords（静态全屏四边形）

**Uniform：** `PostProcessUniforms` — time + screen_dims + enabled

| 方法 | 说明 |
|------|------|
| `set_enabled(bool)` | 启用/禁用后处理 |
| `set_scene_texture(view)` | 设置场景纹理（前序 pass 的输出） |

### DebugOverlayPass（debug_overlay.rs）

调试文本覆盖渲染，使用字形图集，无圆角裁剪。

**顶点格式：** `DebugVertex` — position + tex_coords + color

| 方法 | 说明 |
|------|------|
| `set_atlas_texture(texture)` | 设置调试字形图集 |
| `set_visible(bool)` | 切换可见性 |
| `is_visible()` | 当前可见性 |

## 缓冲区管理

所有 pass 使用预分配缓冲区（最大 100K cells），通过 `update_buffers()` 每帧写入新数据：

- `max_vertex_buffer_size()` = 100,000 × 4 顶点 × 顶点大小
- `max_index_buffer_size()` = 100,000 × 6 索引 × 4 字节

`resize()` 方法在大多数 pass 中为 no-op（预分配足够大），仅在 `PostProcessPass` 中清除场景纹理引用。

## 常见任务

### 添加新的渲染 pass

1. 创建新文件 `render_pass/<name>.rs`
2. 定义顶点格式（实现 Pod/Zeroable）
3. 实现 `RenderPass` trait
4. 在 `mod.rs` 中导出
5. 在 `TerminalRenderer` 中注册到 pass 管线

### 修改现有 pass 的着色器行为

编辑对应 pass 文件中的着色器创建代码。大多数 pass 使用 `create_simple_pipeline()` 创建管线。

### 调试 pass 渲染顺序

在 `TerminalRenderer::draw_to_surface()` 中，各 pass 的 `render()` 调用顺序即为实际渲染顺序。临时注释掉某个 pass 可隔离渲染问题。

## 父级导航

← [pureterm-gui/AGENTS.md](../../AGENTS.md)
