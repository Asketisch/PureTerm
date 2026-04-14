# pureterm-gui — wgpu GPU 渲染 + egui 集成

使用 wgpu 29 实现 GPU 加速的终端文本渲染：字形图集、文本管线、WGSL 着色器。提供 egui-wgpu 集成层用于设置面板覆盖。

## 关键依赖

内部依赖：`pureterm-core`、`pureterm-terminal`

外部依赖：`wgpu 29`（GPU）、`fontdue`（字体光栅化）、`egui 0.34` + `egui-wgpu 0.34` + `egui-winit 0.34`（UI 集成）、`bytemuck 1`（顶点数据转换）、`image 0.25`。

**重要**：workspace 中 `wgpu = "29"`，与 `egui-wgpu 0.34` 使用的 wgpu 版本一致。如果版本不一致会导致运行时 "Mix of internally allocated and externally provided IDs" panic。

## 文件结构

```
src/
├── lib.rs             # 模块导出
├── renderer.rs        # TerminalRenderer（主渲染器）
├── pipeline.rs        # TextPipeline + Vertex（渲染管线）
├── egui_backend.rs    # EguiBackend（egui-wgpu 集成）
├── fonts.rs           # GlyphAtlas（字形图集）
├── egui_int.rs        # egui 集成层（TerminalWidget）
├── transparency.rs    # 透明背景支持
├── background.rs      # 背景渲染
└── shaders/
    └── terminal.wgsl # WGSL 着色器
```

## 渲染流程

```
ScreenSnapshot → 遍历 Cell → 背景四边形 + 字形四边形 → Vertex/Index 缓冲区 → wgpu 渲染
多窗格：每个窗格指定 ViewportRect 偏移 → 合并顶点 → 一次 draw call
设置覆盖：draw_to_surface (LoadOp::Clear) → egui_backend.render (LoadOp::Load) → frame.present()
```

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

**初始化**（`new`，异步）：
1. 创建 wgpu Instance（`InstanceDescriptor` 含 `display: None`、`memory_budget_thresholds`、`backend_options`）
2. 请求高性能 Adapter（返回 `Result` 不是 `Option`，需 `.map_err()`）
3. 创建 Device/Queue（`request_device` 只需一个参数）
4. 初始化 GlyphAtlas（14pt）
5. 加载默认字体

**Surface 配置**（`configure_surface`）：
1. 配置 Surface（启用 VSync）
2. 创建图集纹理（R8Unorm）
3. 创建 TextPipeline

**多窗格渲染**：
- `build_vertices(snapshot, color_scheme, selection, viewport: Option<ViewportRect>)` — 支持 ViewportRect 偏移，窗格渲染到子区域
- `build_rect_vertices(rect, color, ...)` — 渲染填充矩形（分隔线、边框、菜单背景）
- `build_focus_border(rect, color, thickness, ...)` — 4 条细矩形组成焦点边框
- `build_text_vertices(text, x, y, color, ...)` — 用字形图集渲染文本（上下文菜单）
- `draw_to_surface(...)` → 返回 `Option<wgpu::SurfaceTexture>`（不 present），用于后续 egui 覆盖
- `draw(...)` → 完整渲染并 present（兼容旧接口）

**wgpu 29 API 注意**：
- `get_current_texture()` 返回 `CurrentSurfaceTexture` 枚举（`Success`/`Suboptimal`），不是 `Result`
- `ImageCopyTexture` → `TexelCopyTextureInfo`，`ImageDataLayout` → `TexelCopyBufferLayout`

### pipeline.rs — 渲染管线

| 类型 | 说明 |
|------|------|
| `Vertex` | position[2] + tex_coords[2] + color[4]，实现 Pod/Zeroable |
| `TextPipeline` | pipeline + vertex/index 缓冲区 + bind group |

`TextPipeline::new`：
- 创建 shader module（从 WGSL）
- 创建 nearest-neighbor sampler
- 创建 bind group（texture + sampler）
- 创建 render pipeline（alpha blending）
- 预分配缓冲区（最多 1024×1024 个 Cell）

**wgpu 29 API 注意**：
- `PipelineLayoutDescriptor` 使用 `immediate_size` 替代 `push_constant_ranges`
- `RenderPipelineDescriptor` 使用 `multiview_mask` 替代 `multiview`

`TextPipeline::render` — 设置 pipeline、bind group、缓冲区，执行 `draw_indexed`。
`TextPipeline::update_buffers` — 写入顶点和索引数据到 GPU 缓冲区。

### egui_backend.rs — egui-wgpu 集成

| 字段 | 说明 |
|------|------|
| `context` | `egui::Context` |
| `winit_state` | `egui_winit::State`（桥接 winit 事件） |
| `renderer` | `egui_wgpu::Renderer` |

关键方法：
- `new(device, surface_format, window)` — 初始化 egui context、winit state、wgpu renderer
- `on_event(window, event)` — 处理 winit 窗口事件，返回是否被 egui 消费
- `run(raw_input, update_fn)` — 运行 egui 更新，返回 `FullOutput`
- `render(device, queue, surface_view, paint_jobs, textures_delta, screen_descriptor)` — 在现有 surface texture 上用 `LoadOp::Load` 渲染 egui 覆盖

**注意**：`egui_wgpu::Renderer::render()` 需要 `RenderPass<'static>`，用 `forget_lifetime()` 从 `RenderPass<'_>` 转换。

### fonts.rs — 字形图集

| 类型 | 说明 |
|------|------|
| `GlyphInfo` | x、y、width、height、x_offset、y_offset、advance |
| `GlyphAtlas` | 字体列表 + 缓存 + 2048×2048 R8 图集 |

`GlyphAtlas` 关键设计：
- 2048×2048 像素的 R8 纹理图集
- 像素 (0,0) 保留为白色（用于背景四边形渲染）
- 字形缓存：`HashMap<(font_index, char), GlyphInfo>`
- 行式排列：`cursor_x` / `cursor_y` 追踪当前写入位置
- 超出行宽自动换行，图集满时返回空白

`load_default_font()` 平台搜索顺序：
- macOS：SFNSMono → Menlo → JetBrainsMono
- Linux：DejaVuSansMono → LiberationMono

`cell_size()` — 用 'M' 宽度 × 1.2 行高计算。

### egui_int.rs — egui 集成层

提供 egui widget 用于在 egui UI 中嵌入终端：

| 类型 | 说明 |
|------|------|
| `TerminalWidget` | egui Widget，显示终端内容并处理鼠标事件 |

`TerminalWidget` 实现 `egui::Widget` trait，在 egui 布局中预留空间并处理点击/拖拽事件。实际渲染仍由 wgpu 文本管线完成。

### transparency.rs — 透明背景支持

支持终端窗口透明背景渲染，配置窗口透明度级别。

### background.rs — 背景渲染

终端背景渲染模块，支持纹理背景或纯色背景。

### shaders/terminal.wgsl — 着色器

- **顶点着色器**（`vs_main`）：直接传递 position、tex_coords、color
- **片段着色器**（`fs_main`）：采样图集纹理，用 red 通道作为 alpha 遮罩，乘以顶点颜色 alpha：`vec4f(color.rgb, color.a * alpha)`

这个设计使同一个着色器既能渲染实心背景四边形（采样白色像素 0,0，alpha=1.0），也能渲染文字字形（采样字形位图，alpha 由字形形状决定）。

## 常见任务

### 修改渲染效果
编辑 `renderer.rs` 的 `build_vertices` 方法。背景和文字四边形的构建逻辑在此。

### 修改着色器
编辑 `shaders/terminal.wgsl`。注意当前设计依赖 red 通道作 alpha 遮罩的约定。

### 添加字体支持
在 `fonts.rs` 的 `load_default_font` 中添加搜索路径，或通过 `add_font`/`load_font` API 动态加载。

### 调试渲染问题
1. 检查 `Terminal::is_dirty()` 标志是否正确
2. 验证 `ScreenSnapshot` 内容与预期网格状态匹配
3. 检查 `GlyphAtlas::get_glyph()` 缓存是否命中
4. 确认 wgpu 管线缓冲区通过 `update_buffers()` 更新
5. 检查图集是否已满（2048×2048 用尽时字形为空白）

### 修改管线配置
编辑 `pipeline.rs`。注意 alpha blending 设置和预分配缓冲区大小。

### 调试 egui 设置覆盖不显示
1. 确认 `settings_panel.visible` 为 true
2. 确认 `draw_to_surface()` 返回 `Some(frame)`
3. 检查 egui `raw_input` 是否通过 `take_egui_input()` 获取
4. 确认 `frame.present()` 在 egui render 之后调用

### 集成 egui UI
使用 `TerminalWidget::new()` 创建 widget，传入 `ScreenSnapshot` 和 `ColorScheme`。
