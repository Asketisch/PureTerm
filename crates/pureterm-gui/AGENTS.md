# pureterm-gui — wgpu GPU 多 pass 渲染 + egui 集成

使用 wgpu 29 实现 GPU 加速的终端文本渲染。采用多渲染 pass 架构（背景色→装饰→文字→光标→图片→后处理→调试覆盖），字形图集、软件光栅化 Sprite、HarfBuzz 文本整形、ShaderToy 自定义着色器。提供 egui-wgpu 集成层用于设置面板覆盖。

## 关键依赖

内部依赖：`pureterm-core`、`pureterm-terminal`

外部依赖：`wgpu 29`（GPU）、`fontdue`（字体光栅化）、`rustybuzz`（HarfBuzz 文本整形）、`fontdb`（字体发现与回退链）、`ttf-parser`（OpenType 解析）、`egui 0.34` + `egui-wgpu 0.34` + `egui-winit 0.34`（UI 集成）、`bytemuck 1`（顶点数据转换）、`glam 0.29`（矩阵数学）、`image 0.25`（背景图片解码）、`naga 24`（着色器编译）。

**重要**：workspace 中 `wgpu = "29"`，与 `egui-wgpu 0.34` 使用的 wgpu 版本一致。如果版本不一致会导致运行时 "Mix of internally allocated and externally provided IDs" panic。

## 文件结构

```
src/
├── lib.rs             # 模块导出
├── renderer.rs        # TerminalRenderer（主渲染器，组装各 pass）
├── pipeline.rs        # TextPipeline + Vertex（兼容旧接口）
├── render_pass/       # 多 pass 渲染架构
│   ├── mod.rs          # RenderPass trait + PassData + 工厂函数 (226 行)
│   ├── bg_color.rs     # BgColorPass 单元格背景色 (150 行)
│   ├── cell_text.rs    # CellTextPass 字形渲染 (184 行)
│   ├── cursor.rs       # CursorPass 光标 + 闪烁 (203 行)
│   ├── decorations.rs  # DecorationsPass 下划线/删除线/上划线 (148 行)
│   ├── bg_image.rs     # BgImagePass 背景壁纸 (209 行)
│   ├── kitty_image.rs  # KittyImagePass Kitty 内联图片 (184 行)
│   ├── post_process.rs # PostProcessPass 后处理效果 (214 行)
│   ├── debug_overlay.rs # DebugOverlayPass 调试文本覆盖 (164 行)
│   └── AGENTS.md       # 渲染 pass 架构详解
├── render_thread.rs   # RenderThread 独立渲染线程 (259 行)
├── fonts.rs           # GlyphAtlas 字形图集（2048×2048 R8）
├── font_fallback.rs   # FontFallback 字体回退链（fontdb）(273 行)
├── shaping.rs         # TextShaper HarfBuzz 文本整形 (139 行)
├── sprite.rs          # 软件 Sprite 光栅化器（Block/Box/Braille/Powerline）(1433 行)
├── egui_backend.rs    # EguiBackend（egui-wgpu 集成）
├── egui_int.rs        # egui 集成层（TerminalWidget）
├── shader_toy.rs      # ShaderToy 自定义着色器支持 (281 行)
├── transparency.rs    # 透明背景支持
├── background.rs      # 背景渲染
├── math.rs            # GPU 数学工具（正交投影、坐标转换）(18 行)
└── shaders/
    └── terminal.wgsl  # WGSL 着色器
```

## 渲染架构

### 多 pass 管线

渲染从单一管线升级为 8 个独立 pass，每个 pass 负责一类视觉元素：

```
BgImagePass → BgColorPass → DecorationsPass → CellTextPass
    → KittyImagePass → CursorPass → PostProcessPass → DebugOverlayPass
```

每个 pass 实现 `RenderPass` trait：

```rust
trait RenderPass {
    fn label() -> &'static str;
    fn update_buffers(&mut self, device: &Device, queue: &Queue, data: &PassData);
    fn render(&self, pass: &mut RenderPass<'_>);
    fn resize(&mut self, device: &Device, new_size: (u32, u32));
}
```

### 渲染流程

```
ScreenSnapshot → 遍历 Cell → 各 pass 生成顶点 → 按序 render
多窗格：每个窗格指定 ViewportRect 偏移 → 合并顶点 → 一次 draw call
设置覆盖：draw_to_surface (LoadOp::Clear) → egui_backend.render (LoadOp::Load) → frame.present()
```

### 独立渲染线程（render_thread.rs）

`RenderThread` 在独立线程运行，通过消息传递与 UI 线程通信：

| 消息类型 | 说明 |
|----------|------|
| `Frame` | 请求渲染帧 |
| `Resize` | 窗口尺寸变化 |
| `SetBackgroundImage` | 设置背景图片 |
| `ClearBackground` | 清除背景图片 |
| `SetCornerRadius` | 设置圆角半径 |
| `SetCursorBlink` | 设置光标闪烁 |
| `Shutdown` | 关闭渲染线程 |

`RenderStats` 通过原子变量无锁统计 FPS、帧计数、draw call 数和顶点数。

## 模块详情

### renderer.rs — 主渲染器

`TerminalRenderer` 持有完整的 wgpu 资源：

| 字段 | 说明 |
|------|------|
| `instance` | wgpu Instance |
| `adapter` | wgpu Adapter（优先高性能） |
| `device` / `queue` | wgpu Device 和 Queue |
| `atlas` | `GlyphAtlas` 字形图集 |
| `text_pipeline` | `Option<TextPipeline>` 渲染管线 |
| `atlas_texture` | `Option<wgpu::Texture>` 图集纹理 |
| `surface_config` | `Option<wgpu::SurfaceConfiguration>` |
| `transparency_config` | `TransparencyConfig` 透明背景配置 |

**初始化**（`new`，异步）：
1. 创建 wgpu Instance
2. 请求高性能 Adapter
3. 创建 Device/Queue
4. 初始化 GlyphAtlas
5. 加载默认字体

**Surface 配置**（`configure_surface`）：
1. 配置 Surface（启用 VSync），根据 `TransparencyConfig` 设置 alpha 模式
2. 创建图集纹理（R8Unorm）
3. **重要**：创建图集纹理后必须调用 `self.atlas.mark_dirty()` 强制下一帧重新上传字形数据（窗口 resize 会销毁旧纹理，新纹理为空）
4. 创建 TextPipeline
5. 更新管线屏幕尺寸和圆角半径

**多窗格渲染**：
- `build_vertices(snapshot, color_scheme, selection, viewport)` — 支持 ViewportRect 偏移
- `build_rect_vertices(rect, color, ...)` — 渲染填充矩形
- `build_focus_border(rect, color, thickness, ...)` — 焦点边框
- `build_text_vertices(text, x, y, color, ...)` — 用字形图集渲染文本
- `draw_to_surface(...)` → 返回 `Option<wgpu::SurfaceTexture>`（不 present）
- `draw(...)` → 完整渲染并 present

**颜色解析流程**：

1. **raw_fg/raw_bg**：`Rgb` → 直接使用；`Palette(0-7)` + bold → `(idx+8)` 亮色；`Palette(8-15)` → 直接；`Named` → 配色方案默认色
2. **Inverse 模式**：交换 fg ↔ bg
3. **选区覆盖**：背景色与 selection_bg 混合
4. **Hidden 文本**：跳过字形四边形

### render_pass/ — 渲染 pass 架构

详细文档：[render_pass/AGENTS.md](src/render_pass/AGENTS.md)

| Pass | 职责 |
|------|------|
| `BgImagePass` | 全屏背景图片（wallpaper），支持多种缩放模式 |
| `BgColorPass` | 单元格背景色四边形 |
| `DecorationsPass` | 下划线、删除线、上划线装饰 |
| `CellTextPass` | 字形四边形（采样图集纹理） |
| `KittyImagePass` | Kitty 协议内联图片（RGBA 纹理） |
| `CursorPass` | 终端光标（支持闪烁，500ms 切换） |
| `PostProcessPass` | 全屏后处理（暗角、ShaderToy 自定义着色器） |
| `DebugOverlayPass` | 调试文本覆盖（可切换可见性） |

每个 pass 的顶点格式略有不同，但都实现 `RenderPass` trait。缓冲区按最大容量（100K cells）预分配。

### pipeline.rs — 渲染管线（兼容层）

| 类型 | 说明 |
|------|------|
| `Vertex` | position[2] + tex_coords[2] + color[4]，实现 Pod/Zeroable |
| `TerminalUniforms` | corner_radius + screen_width + screen_height |
| `TextPipeline` | pipeline + vertex/index 缓冲区 + uniform 缓冲区 + bind group |

**wgpu 29 API 注意**：
- `ImageCopyTexture` → `TexelCopyTextureInfo`，`ImageDataLayout` → `TexelCopyBufferLayout`

### fonts.rs — 字形图集

| 类型 | 说明 |
|------|------|
| `GlyphInfo` | x、y、width、height、x_offset、y_offset、advance |
| `GlyphAtlas` | 字体列表 + 缓存 + 2048×2048 R8 图集 |

关键设计：
- 2048×2048 像素 R8 纹理图集
- 像素 (0,0) 保留白色（背景四边形渲染用）
- 字形缓存：`HashMap<(font_index, char), GlyphInfo>`
- 行式排列，超出行宽自动换行
- `cell_size()` — 'M' 宽度 × 1.2 行高
- `mark_dirty()` — 标记图集为脏，强制下一帧重新上传到 GPU（窗口 resize 后纹理重建时调用）

### font_fallback.rs — 字体回退链

使用 `fontdb` 实现五步字体回退策略：

| 步骤 | 策略 |
|------|------|
| 1 | 等宽字体族（family monospace） |
| 2 | Emoji 字体（平台特定） |
| 3 | CJK 字体（中日韩统一表意文字） |
| 4 | 无衬线字体（sans-serif） |
| 5 | 衬线字体（serif，最后手段） |

| 类型 | 说明 |
|------|------|
| `FallbackFont` | 解析结果：path、face_index、raw data、is_color |
| `FontFallback` | 解析器，包装 fontdb Database + 缓存 |

辅助函数：`is_emoji()`（20+ Unicode 范围）、`is_cjk()`、`is_color_font()`（检测 CBDT/CBLC/sbix/COLR 表）。

### shaping.rs — HarfBuzz 文本整形

使用 `rustybuzz` 实现复杂脚本整形：

| 类型 | 说明 |
|------|------|
| `ShapedGlyph` | glyph_id、偏移、advance、is_mark、cluster |
| `TextShaper` | 无状态整形器包装 |

`TextShaper::shape()` 返回视觉序 glyph 列表，支持 BiDi、Arabic、Indic、emoji ZWJ 序列。

### sprite.rs — 软件 Sprite 光栅化器（1433 行）

绕过字体光栅化，直接绘制终端专用字符的像素模式：

| 函数 | 处理范围 |
|------|----------|
| `draw_block_element()` | U+2580-U+259F（半块、阴影、象限） |
| `draw_box_drawing()` | U+2500-U+257F（线条、角、T 形、交叉、双线、对角线、弧线） |
| `draw_braille()` | U+2800-U+28FF（2×4 点阵，按 bit 模式） |
| `draw_powerline()` | U+E0B0-U+E0BF（三角形、火焰、箭头、git 符号） |
| `CursorStyle::draw()` | 光标形状（Block/Underline/Beam） |

谓词函数：`is_block_element()`、`is_box_drawing()`、`is_braille()`、`is_powerline()`。

### shader_toy.rs — ShaderToy 自定义着色器

支持加载 GLSL ES 3.0 自定义后处理着色器：

| 类型 | 说明 |
|------|------|
| `ShaderToyUniforms` | ShaderToy 标准 uniform（time/resolution/frame/mouse/date） |
| `ShaderPass` | 单个着色器 pass（GLSL 源码 + channel 绑定） |
| `ShaderToy` | 管理器：pass 列表、ping-pong 缓冲纹理、帧状态 |

内置暗角着色器（`DEFAULT_VIGNETTE_GLSL`）。支持多 pass 渲染到缓冲纹理。

### egui_backend.rs — egui-wgpu 集成

| 字段 | 说明 |
|------|------|
| `context` | `egui::Context` |
| `winit_state` | `egui_winit::State` |
| `renderer` | `egui_wgpu::Renderer` |

关键方法：`new()`、`on_event()`、`run()`、`render()`（`LoadOp::Load` 覆盖）。

**注意**：`egui_wgpu::Renderer::render()` 需要 `RenderPass<'static>`，用 `forget_lifetime()` 转换。

### egui_int.rs — egui 集成层

`TerminalWidget` 实现 `egui::Widget` trait，在 egui 布局中预留空间并处理鼠标事件。实际渲染由 wgpu 文本管线完成。

### transparency.rs — 透明背景支持

| 类型 | 说明 |
|------|------|
| `TransparencyConfig` | opacity（0.0-1.0）、blur、blur_radius、transparent_background |

当 `opacity < 1.0` 或 `transparent_background` 为 true 时启用透明渲染。

### background.rs — 背景渲染

| 类型 | 说明 |
|------|------|
| `BackgroundConfig` | image_path、mode（Stretch/Tile/Center/Cover/Fit）、opacity、blur |
| `BackgroundMode` | 5 种缩放模式 |

### math.rs — GPU 数学工具

| 函数 | 说明 |
|------|------|
| `orthographic(width, height)` | 正交投影矩阵（`glam::Mat4`），像素坐标 → NDC |
| `pixel_to_ndc(x, y, screen_w, screen_h)` | 单点像素 → NDC 转换 |

## 常见任务

### 修改渲染效果
编辑 `renderer.rs` 的 `build_vertices` 方法。

### 添加新的渲染 pass
1. 在 `render_pass/` 下创建新文件
2. 实现 `RenderPass` trait
3. 在 `renderer.rs` 中注册到 pass 管线
4. 在 `render_pass/mod.rs` 中导出

### 修改着色器
编辑 `shaders/terminal.wgsl`。注意依赖 red 通道作 alpha 遮罩的约定。

### 添加字体回退
编辑 `font_fallback.rs` 的 `EMOJI_FAMILIES` / `CJK_FAMILIES` 常量。

### 添加 Sprite 字符
在 `sprite.rs` 中添加新的绘制函数和对应的 Unicode 范围检测。

### 调试渲染问题
1. 检查 `Terminal::is_dirty()` 标志
2. 验证 `ScreenSnapshot` 内容
3. 检查 `GlyphAtlas::get_glyph()` 缓存命中
4. 确认 wgpu 管线缓冲区通过 `update_buffers()` 更新
5. 检查图集是否已满（2048×2048）
6. 使用 `DebugOverlayPass` 可视化调试信息

### 集成 egui UI
使用 `TerminalWidget::new()` 创建 widget，传入 `ScreenSnapshot` 和 `ColorScheme`。
