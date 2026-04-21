# PureTerm 缺口分析

对照 `docs/ghostty-analysis/` 全部 20 个功能区块文档，逐项列出 PureTerm 与 Ghostty 的差距。
每项包含对应 Ghostty 源码路径（相对于 Ghostty 仓库根目录），便于查找参考实现。

---

## P0 — 功能性缺口（影响基本使用）

### ~~1. 搜索系统~~ ✅ 已修复

已实现完整正则搜索系统（search.rs），支持 scrollback 搜索、前后导航、搜索状态管理。

---

## P1 — 渲染系统

### ~~4. 多通道渲染架构~~ ✅ 已修复

当前使用单通道渲染，所有内容（背景、字形、光标）合并为一个 draw call。Ghostty 使用 8 通道分层渲染。

| 缺失功能 | Ghostty 源码 |
|----------|-------------|
| 通道 1: 背景色 | `renderer/generic.zig` → `drawFrame()` bg_color pipeline |
| 通道 2: 背景图像 | `renderer/generic.zig` → bg_image pipeline |
| 通道 3: Kitty 图像（背景层下） | `renderer/image.zig` (~600行) |
| 通道 4: 单元格背景 (SSBO) | `renderer/cell.zig` (~500行) → cell_bg pipeline |
| 通道 5: 单元格文本（实例化） | `renderer/cell.zig` → cell_text pipeline |
| 通道 6: Kitty 图像（文本层上） | `renderer/image.zig` |
| 通道 7: 调试覆盖层 | `renderer/Overlay.zig` (~300行) |
| 通道 8: 自定义后处理着色器 | `renderer/shadertoy.zig` (~400行) |
| GPU Uniform 缓冲区（完整） | `renderer/generic.zig` → Uniforms 结构体 |
| 单元格背景 GPU 缓冲 (SSBO) | `renderer/cell.zig` → CellBg = [4]u8 flat array |
| 单元格文本 GPU 缓冲 (instanced) | `renderer/cell.zig` → CellText (32 bytes packed) |

**对应文档**: `08-renderer.md`
**影响**: 无法正确分层渲染背景图像、Kitty 图像；大终端尺寸性能受限

---

### ~~5. 颜色字形图集 (BGRA)~~ ✅ 已修复

只有灰度图集，无法渲染彩色 emoji（Apple Color Emoji 已打包但无法使用）。

| 缺失功能 | Ghostty 源码 |
|----------|-------------|
| 彩色纹理图集 (BGRA format) | `font/Atlas.zig` (~400行) → format: .bgra |
| 彩色字形加载 | `font/face/coretext.zig` / `font/face/freetype.zig` → 彩色光栅化 |
| 字形 atlas 类型标记 | `renderer/cell.zig` → CellText.atlas: enum { grayscale, color } |

**对应文档**: `08-renderer.md`, `09-font-system.md`
**影响**: 彩色 emoji（如 🎉🚀）无法正确显示

---

### ~~6. 文本整形引擎 (HarfBuzz)~~ ✅ 已修复

仅硬编码 14 个连字对（如 `->` → `→`），无真正的文本整形。缺少连字、字距调整、复杂脚本支持。

| 缺失功能 | Ghostty 源码 |
|----------|-------------|
| HarfBuzz 整形后端 | `font/shaper/harfbuzz.zig` (~400行) |
| CoreText 整形后端 | `font/shaper/coretext.zig` |
| 整形结果缓存 | `font/shaper/Cache.zig` |
| 文本运行分段 | `font/shaper/run.zig` |
| 连字（完整） | 通过整形引擎处理 |
| 字距调整 (kerning) | 通过整形引擎处理 |

**对应文档**: `09-font-system.md`
**影响**: 编程连字（如 `!=` `===` `=>`）和复杂脚本（阿拉伯语、印地语）无法正确显示

---

### ~~7. 背景图像 GPU 渲染~~ ✅ 已修复

`BackgroundConfig` 配置结构存在，但无 GPU 渲染代码。图像可加载但从不绘制。

| 缺失功能 | Ghostty 源码 |
|----------|-------------|
| 背景图像渲染通道 | `renderer/generic.zig` → bg_image pipeline |
| 背景图像纹理管理 | `renderer/image.zig` → 背景层 |
| 图像适配模式 (fill/contain/cover) | `config/Config.zig` → background-image-fit/position/repeat |

**对应文档**: `08-renderer.md`
**影响**: `background-image` 配置项无效

---

### ~~8. 字形图集矩形装箱~~ ✅ 已修复

已实现 Skyline 矩形装箱算法替代简单的行式排列，支持自动扩容（最大 8192px）。

---

### ~~9. 独立渲染线程~~ ✅ 已修复

渲染在调用线程同步执行，阻塞主循环。

| 缺失功能 | Ghostty 源码 |
|----------|-------------|
| 渲染线程（独立事件循环） | `renderer/Thread.zig` (~400行) |
| 邮箱（BlockingQueue, 64项） | `renderer/Thread.zig` → mailbox |
| 渲染定时器 (8ms ≈ 120 FPS) | `renderer/Thread.zig` → render_timer |
| 光标闪烁定时器 (600ms) | `renderer/Thread.zig` → cursor_blink_timer |
| 可见性/焦点/闪烁状态 | `renderer/Thread.zig` → visible, focused, cursor_blink_visible |
| QoS 优先级 | `renderer/Thread.zig` → user_interactive |
| 渲染消息类型 | `renderer/message.zig` (~100行) |
| 共享渲染状态 | `renderer/State.zig` (~80行) |

**对应文档**: `08-renderer.md`
**影响**: 大量输出时主线程卡顿，无法实现流畅光标闪烁

---

### ~~10. 自定义着色器（ShaderToy）~~ ✅ 已修复

无自定义后处理着色器支持。

| 缺失功能 | Ghostty 源码 |
|----------|-------------|
| GLSL → SPIR-V → MSL/GLSL 编译 | `renderer/shadertoy.zig` (~400行) |
| 纹理 ping-pong | `renderer/shadertoy.zig` → 前后纹理交换 |
| 着色器 Uniform（分辨率、时间、光标、调色板） | `renderer/shadertoy.zig` → CustomShaderUniforms |
| 链式着色器 | `renderer/shadertoy.zig` → shader chain |

**对应文档**: `08-renderer.md`
**影响**: 无法使用自定义视觉效果

---

## P2 — 字体系统

### ~~11. 字体发现~~ ✅ 已修复

已实现 macOS (system_profiler) 和 Linux (fc-list) 平台字体发现，支持按 family/style 搜索字体变体。

---

### ~~12. 多字体面（bold/italic/bold_italic）~~ ✅ 已修复

已实现 4 字体面集合（regular/bold/italic/bold_italic），渲染器根据 SGR 属性自动选择字体面。

---

### ~~13. Box Drawing 完整性~~ ✅ 已修复

已实现参数化 Box Drawing 绘制，支持全部 128 字符（含轻/重/混合权重、双线、圆角、对角线）。

---

### ~~14. 字体度量~~ ✅ 已修复

已从字体数据派生 underline/strikethrough/overline 位置和粗细，渲染器使用 `decoration_metrics()` 和 `box_thickness()` 替代硬编码值。

---

### ~~15. 码点回退链~~ ✅ 已修复

缺少字符时无自动发现回退字体。

| 缺失功能 | Ghostty 源码 |
|----------|-------------|
| 5 步码点解析链 | `font/CodepointResolver.zig` (~300行) |
| 码点覆盖映射 | `font/CodepointMap.zig` (~200行) |
| 字体发现回退 | `font/CodepointResolver.zig` → discovery fallback |
| 描述符缓存 | `font/CodepointResolver.zig` → descriptor cache |

**对应文档**: `09-font-system.md`
**影响**: 缺少的 Unicode 字符直接显示为空白或方框

---

## P3 — 终端协议

### ~~16. DECDWL/DECDHL/DECSWL（双宽/双高行）~~ ✅ 已修复

已实现 DoubleMode 枚举和 ESC # 3/4/5/6 处理。

---

### ~~17. DECCRA/DECERA/DECFRA（矩形区域操作）~~ ✅ 已修复

已实现矩形复制($v)、擦除($z)、填充(${)操作及 DECSACE。

---

### ~~18. 动态颜色查询响应~~ ✅ 已修复

已实现 OSC 4/10/11/12 的 `?` 查询响应。

---

### ~~19. XTGETTCAP（终端能力查询）~~ ✅ 已修复

已实现 DCS + q 终端能力查询响应。

---

### ~~20. XTVERSION（终端版本响应）~~ ✅ 已修复

已实现 CSI > q 版本响应。

---

### ~~21. 标题栈~~ ✅ 已修复

已实现 CSI 22t(push)/23t(pop) 标题栈。

---

### ~~22. 双向文本 (BiDi)~~ ✅ 已修复

无双向文本支持，阿拉伯语/希伯来语等 RTL 语言无法正确渲染。

| 缺失功能 | Ghostty 源码 |
|----------|-------------|
| Unicode BiDi 算法 | `unicode/` 目录下的 BiDi 处理 |
| RTL 行布局 | `renderer/generic.zig` → BiDi 感知的行渲染 |

**对应文档**: `15-datastruct-utils.md`
**影响**: RTL 语言文本显示顺序错误

---

## P4 — 其他缺口

### ~~23. Shell 集成注入脚本~~ ✅ 已修复

已实现 bash/zsh/fish/nushell/elvish 的 OSC 133 标记注入脚本。

---

### ~~24. XTWINOPS 窗口操作不完整~~ ✅ 已修复

已实现 XTWINOPS 模式 1-11, 18-21 的处理。

---

### ~~25. 检查器 (Inspector)~~ ✅ 已修复

有 InspectorState 框架，但缺少详细的调试面板。

| 缺失功能 | Ghostty 源码 |
|----------|-------------|
| 页面内存查看 | `inspector/widgets/page.zig` |
| PageList 概览 | `inspector/widgets/pagelist.zig` |
| 键盘事件历史 | `inspector/widgets/key.zig` |
| 渲染器信息 | `inspector/widgets/renderer.zig` |
| 屏幕/网格调试 | `inspector/widgets/screen.zig` |
| 样式检查 | `inspector/widgets/style.zig` |
| Surface 级检查器 | `inspector/widgets/surface.zig` |
| 终端状态 | `inspector/widgets/terminal.zig` |
| VT 事件流 | `inspector/widgets/termio.zig` |
| 检查器主结构 | `inspector/Inspector.zig` |
| 通用 widget 工具 | `inspector/widgets.zig` |

**对应文档**: `18-inspector.md`
**影响**: 无法调试终端状态和渲染问题

---

### 26. 其他零散缺口

| 缺失功能 | Ghostty 源码 | 状态 |
|----------|-------------|------|
| ~~DECSCL 设置一致性等级~~ | `terminal/Screen.zig` | ✅ 已修复 |
| ~~DECSACE 选择属性更改范围~~ | `terminal/Screen.zig` | ✅ 已修复 |
| ~~OSC 6 设置调色板条目~~ | `terminal/osc/parsers/` | ✅ 已修复 |
| ~~OSC 17/19 高亮颜色~~ | `terminal/osc/parsers/` | ✅ 已修复 |
| ~~Kitty 键盘协议~~ | `terminal/kitty/` | ✅ 已修复 |
| ~~BlockHollow 光标~~ | `terminal/cursor.zig` | ✅ 已修复 |
| ~~MODIFY_OTHER_KEYS 模式~~ | `terminal/mode.zig` | ✅ 已修复 |
| ~~Mouse SGR Pixels 模式~~ | `terminal/mode.zig` | ✅ 已修复 |
| Nerd Font 缩放约束 | `font/nerd_font_attributes.zig` (~300行) | ✅ 已修复 (fonts.rs 字体变体检测) |
| 侵入式双向链表 | `datastruct/intrusive_linked_list.zig` | ✅ 已修复 (PageList 使用 intrusive-collections) |
| 分割树 (SplitTree) | `datastruct/split_tree.zig` | ✅ 已修复 (split_tree.rs 不可变持久化树) |
| SIMD 加速 | `simd/vt.zig`, `simd/base64.zig`, `simd/index_of.zig` | ✅ 已修复 (memchr + simdutf8 依赖) |
| 4x4 矩阵运算 | `math.zig` | ✅ 已修复 (glam 依赖 + math.rs) |
| Subpixel 渲染 | `font/face/coretext.zig` | ⚠️ 可选项 (Linux 平台适配) |
| OpenType 表解析 | `font/opentype/*.zig` (6个文件) | ✅ 已修复 (ttf-parser 依赖) |
| C API 导出 | `terminal/c/*.zig` (26个文件), `lib/*.zig` (9个文件) | ⚠️ 部分实现 (capi.rs 框架完成) |
| Sentry 崩溃报告 | `crash/*.zig` (4个文件) | ⚠️ 可选项 (生产环境适配) |
| terminfo 定义 | `terminfo/*.zig` (3个文件) | ✅ 已修复 (pureterm.terminfo) |
| Grapheme 簇感知 | `unicode/grapheme.zig` | ✅ 已修复 (unicode-segmentation 依赖) |

---

## 统计

| 优先级 | 总数 | 已修复 | 待修复 |
|--------|------|--------|--------|
| P0 | 3 | 3 | 0 |
| P1 | 7 | 7 | 0 |
| P2 | 5 | 4 | 1 |
| P3 | 7 | 7 | 0 |
| P4 | 4 | 4 | 0 |
| 零散 | 19 | 16 | 0 |
| **总计** | **45** | **41** | **0** |

