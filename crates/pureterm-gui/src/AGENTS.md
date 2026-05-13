# pureterm-gui/src — GPU 渲染与文本整形

wgpu GPU 加速终端渲染、字形图集、字体回退、文本整形、Sprite 光栅化、egui 集成。

## 文件结构

```
src/
├── lib.rs             (18)    # crate 入口，re-export TerminalRenderer, TerminalWidget, EguiBackend
├── renderer.rs        (1220)  # TerminalRenderer wgpu 主渲染器，多窗格顶点构建
├── pipeline.rs        (283)   # TextPipeline + Vertex + TerminalUniforms
├── render_thread.rs   (258)   # RenderThread 独立渲染线程，消息传递
├── fonts.rs           (1224)  # GlyphAtlas 字形图集（2048x2048 R8），字形缓存和光栅化
├── font_fallback.rs   (272)   # 5 步字体回退链（fontdb），is_emoji/is_cjk 检测
├── shaping.rs         (138)   # TextShaper HarfBuzz 文本整形（rustybuzz）
├── sprite.rs          (1430)  # 软件 Sprite 光栅化器（Block/Box/Braille/Powerline）
├── egui_backend.rs    (189)   # egui 平台后端（context、winit_state、renderer）
├── egui_int.rs        (68)    # egui TerminalWidget 集成
├── shader_toy.rs      (280)   # ShaderToy 自定义着色器（GLSL ES 3.0）
├── transparency.rs    (89)    # 透明背景支持（TransparencyConfig）
├── background.rs      (107)   # 背景渲染（BackgroundConfig、BackgroundMode）
├── math.rs            (18)    # GPU 数学工具（正交投影、像素->NDC 转换）
└── render_pass/               # 多 pass 渲染架构
    ├── mod.rs         (225)   # RenderPass trait + PassData + 工厂函数
    ├── bg_color.rs    (148)   # BgColorPass 单元格背景色
    ├── bg_image.rs    (208)   # BgImagePass 背景壁纸
    ├── cell_text.rs   (182)   # CellTextPass 字形渲染
    ├── cursor.rs      (201)   # CursorPass 光标 + 闪烁
    ├── decorations.rs (146)   # DecorationsPass 下划线/删除线/上划线
    ├── kitty_image.rs (183)   # KittyImagePass Kitty 内联图片
    ├── post_process.rs(212)   # PostProcessPass 后处理效果
    └── debug_overlay.rs(162)  # DebugOverlayPass 调试文本覆盖
```

## 模块依赖

```
lib.rs
 ├── renderer.rs ──── pipeline.rs ──── math.rs
 │    ├── fonts.rs ──── font_fallback.rs
 │    ├── shaping.rs
 │    ├── sprite.rs
 │    ├── background.rs
 │    ├── transparency.rs
 │    ├── shader_toy.rs
 │    └── render_pass/
 │         ├── bg_color.rs
 │         ├── bg_image.rs
 │         ├── cell_text.rs
 │         ├── cursor.rs
 │         ├── decorations.rs
 │         ├── kitty_image.rs
 │         ├── post_process.rs
 │         └── debug_overlay.rs
 ├── render_thread.rs ──── renderer.rs
 ├── egui_backend.rs
 └── egui_int.rs
```

## 模块详情

### lib.rs — crate 入口

导出三个公共类型：`TerminalRenderer`（主渲染器）、`TerminalWidget`（egui 集成）、`EguiBackend`（egui-wgpu 桥接）。声明全部 14 个子模块。

### renderer.rs — TerminalRenderer 主渲染器（1220 行）

`TerminalRenderer` 持有完整的 wgpu 资源，是渲染系统的顶层入口。

| 字段 | 说明 |
|------|------|
| `instance` | wgpu Instance |
| `adapter` | wgpu Adapter（优先高性能） |
| `device` / `queue` | wgpu Device 和 Queue |
| `atlas` | GlyphAtlas 字形图集 |
| `text_pipeline` | Option<TextPipeline> 渲染管线 |
| `atlas_texture` | Option<wgpu::Texture> 图集纹理 |
| `surface_config` | Option<wgpu::SurfaceConfiguration> |
| `transparency_config` | TransparencyConfig 透明背景配置 |

核心方法：

| 方法 | 说明 |
|------|------|
| `new(custom_font_path, font_size)` | 异步初始化：创建 wgpu 资源链、加载字体 |
| `configure_surface()` | 配置 wgpu Surface（启用 VSync）、创建图集纹理 |
| `build_vertices(snapshot, color_scheme, selection, viewport)` | 从 ScreenSnapshot 生成各 pass 渲染顶点 |
| `build_rect_vertices(rect, color, ...)` | 渲染填充矩形 |
| `build_rounded_rect_vertices(...)` | 渲染圆角矩形（SDF shape_type=1） |
| `build_circle_vertices(...)` | 渲染圆形（SDF shape_type=2） |
| `build_focus_border(rect, color, thickness, ...)` | 焦点边框 |
| `build_text_vertices(text, x, y, color, ...)` | 用字形图集渲染文本 |
| `draw_to_surface(...)` | 渲染到 Surface（不 present，供 egui 叠加） |
| `draw(...)` | 完整渲染并 present |

颜色解析流程：Rgb 直接使用；Palette(0-7) + bold 映射亮色；Named 使用配色方案；Inverse 交换 fg/bg；Hidden 跳过字形。

### pipeline.rs — 渲染管线（283 行）

| 类型 | 说明 |
|------|------|
| `Vertex` | position[2] + tex_coords[2] + color[4] + shape_params[4]，实现 Pod/Zeroable |
| `TerminalUniforms` | corner_radius + screen_width + screen_height |
| `TextPipeline` | wgpu pipeline + vertex/index 缓冲区 + uniform 缓冲区 + bind group |

`shape_params` 字段区分形状类型：0=字形/纯色矩形、1=圆角矩形 SDF、2=圆形 SDF。

### render_thread.rs — 独立渲染线程（258 行）

`RenderThread` 在独立线程运行 wgpu 渲染，通过消息传递与 UI 线程通信。

| 消息类型 | 说明 |
|----------|------|
| `Frame` | 请求渲染帧 |
| `Resize` | 窗口尺寸变化 |
| `SetBackgroundImage` | 设置背景图片 |
| `ClearBackground` | 清除背景图片 |
| `SetCornerRadius` | 设置圆角半径 |
| `SetCursorBlink` | 设置光标闪烁 |
| `Shutdown` | 关闭渲染线程 |

`RenderStats` 通过原子变量无锁统计 FPS、帧计数、draw call 数和顶点数。`RenderThreadHandle` 可克隆，允许多个发送者。

### fonts.rs — 字形图集与连字引擎（1224 行）

| 类型 | 说明 |
|------|------|
| `GlyphInfo` | 字形信息：x、y、width、height、x_offset、y_offset、advance、is_ligature、ligature_len |
| `GlyphAtlas` | 字体列表 + 缓存 + 2048x2048 R8 图集 |

图集设计：
- 2048x2048 像素 R8 纹理
- 像素 (0,0) 保留白色（背景四边形用）
- 字形缓存：HashMap<(font_index, char), GlyphInfo>
- 行式排列，超出行宽自动换行
- `mark_dirty()` 标记图集脏，强制下一帧重新上传（窗口 resize 后纹理重建时调用）

连字引擎支持编程连字映射（->, <-, =>, <=, ==, != 等），嵌入 Nerd Font 覆盖 Box-drawing、Block Elements、Powerline。

`cell_size()` 返回 'M' 宽度 x 1.2 行高。

### font_fallback.rs — 5 步字体回退链（272 行）

使用 `fontdb` 的字体回退解析器。

| 类型 | 说明 |
|------|------|
| `FallbackFont` | 回退结果：path、face_index、data、is_color |
| `FontFallback` | 解析器：fontdb Database + loaded_fonts 缓存 |

5 步回退策略：

| 步骤 | 策略 | 目标 |
|------|------|------|
| 1 | 主字体族（monospace） | 等宽字体 |
| 2 | 系统字体（匹配码点） | 通用回退 |
| 3 | Emoji 字体（平台特定） | 彩色表情符号 |
| 4 | CJK 字体（中日韩） | 中文/日文/韩文 |
| 5 | Sans-serif/Serif | 最后手段 |

辅助函数：`is_emoji()`（20+ Unicode 范围）、`is_cjk()`、`is_color_font()`（检测 CBDT/CBLC/sbix/COLR 表）。

### shaping.rs — HarfBuzz 文本整形（138 行）

| 类型 | 说明 |
|------|------|
| `ShapedGlyph` | 整形结果：glyph_id、x/y_offset、x/y_advance、is_mark、cluster |
| `TextShaper` | 无状态整形器包装 |

`TextShaper::shape(font_data, font_index, text, font_size, rtl)` 返回视觉序 glyph 列表，支持 BiDi、Arabic、Indic、emoji ZWJ 序列。

### sprite.rs — 软件 Sprite 光栅化器（1430 行）

绕过字体光栅化，直接绘制终端专用字符的像素模式。

| 函数 | 处理范围 | 说明 |
|------|----------|------|
| `draw_block_element()` | U+2580-U+259F | 半块、阴影、象限字符 |
| `draw_box_drawing()` | U+2500-U+257F | 线条、角、T 形、交叉、双线、对角线、弧线 |
| `draw_braille()` | U+2800-U+28FF | 2x4 点阵，按 bit 模式 |
| `draw_powerline()` | U+E0B0-U+E0BF | 三角形、火焰、箭头、git 符号 |
| `CursorStyle::draw()` | — | 光标形状（Block/Underline/Beam） |

谓词函数：`is_block_element()`、`is_box_drawing()`、`is_braille()`、`is_powerline()`。

### egui_backend.rs — egui-wgpu 平台后端（189 行）

| 字段 | 说明 |
|------|------|
| `context` | egui::Context |
| `winit_state` | egui_winit::State |
| `renderer` | egui_wgpu::Renderer |

| 方法 | 说明 |
|------|------|
| `new(device, surface_format, window)` | 创建 egui 后端 |
| `on_event(event)` | 处理 winit 窗口事件 |
| `run(ctx, ui_fn)` | 运行 egui UI |
| `render(encoder, texture_view)` | 渲染 egui 内容（LoadOp::Load 覆盖） |

包含 CJK 字体族列表（macOS/Linux/Windows 平台特定）。

### egui_int.rs — egui 集成 Widget（68 行）

`TerminalWidget` 实现 `egui::Widget` trait，在 egui 布局中预留终端空间并处理鼠标事件。实际渲染由 wgpu 文本管线完成。

### shader_toy.rs — ShaderToy 自定义着色器（280 行）

| 类型 | 说明 |
|------|------|
| `ShaderToyUniforms` | ShaderToy 标准 uniform：time、resolution、frame、time_delta、frame_rate、sample_rate、date、mouse |
| `ShaderPass` | 单个着色器 pass：GLSL 源码 + channel 绑定 |
| `ShaderToy` | 管理器：pass 列表、ping-pong 缓冲纹理、帧状态 |

内置暗角着色器（`DEFAULT_VIGNETTE_GLSL`），支持多 pass 渲染到缓冲纹理。

### transparency.rs — 窗口透明度（89 行）

| 类型 | 说明 |
|------|------|
| `TransparencyConfig` | 配置：opacity（0.0-1.0）、blur、blur_radius（默认 20px）、transparent_background |

当 `opacity < 1.0` 或 `transparent_background` 为 true 时启用透明渲染。

### background.rs — 背景渲染（107 行）

| 类型 | 说明 |
|------|------|
| `BackgroundConfig` | 配置：image_path、mode、opacity、blur |
| `BackgroundMode` | Stretch / Tile / Center / Cover / Fit |

默认不透明度 0.3，默认缩放模式 Cover。

### math.rs — GPU 数学工具（18 行）

| 函数 | 说明 |
|------|------|
| `orthographic(width, height)` | 正交投影矩阵（glam::Mat4），像素坐标到 NDC |
| `pixel_to_ndc(x, y, screen_w, screen_h)` | 单点像素到 NDC [-1, 1] 转换 |

使用 `glam::Mat4::orthographic_lh`，Y 轴向下。

## 子模块

| 目录 | 描述 | 详细指南 |
|------|------|----------|
| render_pass/ | 8 pass GPU 渲染架构（背景色->装饰->文字->光标->图片->后处理->调试） | [render_pass/AGENTS.md](render_pass/AGENTS.md) |

## 数据目录

| 目录 | 描述 |
|------|------|
| shaders/ | WGSL 着色器文件，各 pass 对应独立 .wgsl：bg_color.wgsl、bg_image.wgsl、cell_text.wgsl、cursor.wgsl、decorations.wgsl、kitty_image.wgsl、post_process.wgsl、debug_overlay.wgsl + 通用 terminal.wgsl |

## 常见任务

### 修改渲染效果
编辑 `renderer.rs` 的 `build_vertices` 方法，调整颜色解析或顶点生成逻辑。

### 添加新的渲染 pass
1. 在 `render_pass/` 下创建新文件，实现 `RenderPass` trait（label、update_buffers、render、resize）
2. 在 `render_pass/mod.rs` 中导出新 pass
3. 在 `renderer.rs` 中注册到 pass 管线
4. 在 `shaders/` 下创建对应的 WGSL 着色器

### 修改着色器
编辑 `shaders/` 下对应的 `.wgsl` 文件。注意依赖 red 通道作 alpha 遮罩的约定。

### 添加字体回退
编辑 `font_fallback.rs` 的 `EMOJI_FAMILIES` / `CJK_FAMILIES` 常量，增加平台特定字体族。

### 添加 Sprite 字符
在 `sprite.rs` 中添加新的绘制函数和对应的 Unicode 范围检测谓词函数。

### 调试渲染问题
1. 检查 `Terminal::is_dirty()` 标志
2. 验证 `ScreenSnapshot` 内容
3. 检查 `GlyphAtlas::get_glyph()` 缓存命中
4. 确认 wgpu 管线缓冲区通过 `update_buffers()` 更新
5. 检查图集是否已满（2048x2048）
6. 使用 `DebugOverlayPass` 可视化调试信息

### 集成 egui UI
使用 `TerminalWidget::new()` 创建 widget，传入 `ScreenSnapshot` 和 `ColorScheme`。

### 添加新的背景模式
在 `background.rs` 的 `BackgroundMode` 枚举中添加变体，在渲染器中实现对应的缩放逻辑。

## 父级导航

<- [pureterm-gui/AGENTS.md](../AGENTS.md)
