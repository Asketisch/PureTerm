# pureterm-gui — wgpu GPU 渲染

使用 wgpu 实现 GPU 加速的终端文本渲染：字形图集、文本管线、WGSL 着色器。

## 关键依赖

内部依赖：`pureterm-core`、`pureterm-terminal`

外部依赖：`wgpu`（GPU）、`fontdue`（字体光栅化）、`egui` + `egui-wgpu`（UI 集成）、`bytemuck 1`（顶点数据转换）、`image 0.25`。

## 文件结构

```
src/
├── lib.rs           # 模块导出
├── renderer.rs      # TerminalRenderer（主渲染器）
├── pipeline.rs      # TextPipeline + Vertex（渲染管线）
├── fonts.rs         # GlyphAtlas（字形图集）
└── shaders/
    └── terminal.wgsl # WGSL 着色器
```

## 渲染流程

```
ScreenSnapshot → 遍历 Cell → 背景四边形 + 字形四边形 → Vertex/Index 缓冲区 → wgpu 渲染
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
1. 创建 wgpu Instance
2. 请求高性能 Adapter
3. 创建 Device/Queue
4. 初始化 GlyphAtlas（14pt）
5. 加载默认字体

**Surface 配置**（`configure_surface`）：
1. 配置 Surface（启用 VSync）
2. 创建图集纹理（R8Unorm）
3. 创建 TextPipeline

**渲染**（`render`）：
1. 上传图集纹理数据到 GPU
2. 遍历 snapshot 中所有 Cell，为每个 Cell 生成：
   - 背景四边形（采样保留的白色像素 0,0，颜色 = 背景色）
   - 字形四边形（从图集采样，UV 坐标指向字形位图）
3. 颜色解析：`Color::Rgb` 直接使用，`Color::Palette` 从 ColorScheme 的 16 色查找，NamedColor 从 ColorScheme 获取
4. 绘制光标（半透明四边形）
5. 更新顶点/索引缓冲区
6. 通过 pipeline 渲染

**注意**：wgpu 24 API 使用 `Option::ok()` 而非 `.map_err()` 来选择 adapter。

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

`TextPipeline::render` — 设置 pipeline、bind group、缓冲区，执行 `draw_indexed`。

`TextPipeline::update_buffers` — 写入顶点和索引数据到 GPU 缓冲区。

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

### shaders/terminal.wgsl — 着色器

- **顶点着色器**（`vs_main`）：直接传递 position、tex_coords、color
- **片段着色器**（`fs_main`）：采样图集纹理，用 red 通道作为 alpha 遮罩，乘以顶点颜色 alpha：`vec4f(color.rgb, color.a * alpha)`

这个设计使同一个着色器既能渲染实心背景四边形（采样白色像素 0,0，alpha=1.0），也能渲染文字字形（采样字形位图，alpha 由字形形状决定）。

## 常见任务

### 修改渲染效果
编辑 `renderer.rs` 的 `render` 方法。背景和文字四边形的构建逻辑在此。

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
