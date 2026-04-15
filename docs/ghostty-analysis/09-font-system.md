# 09 - 字体系统

## 功能概述

完整的字体管理子系统：字体发现、加载、光栅化、文本整形（shaping）、OpenType 解析、图集管理、精灵字体（box drawing/block elements/cursors）、Nerd Font 支持。

## Ghostty 源码文件

### 核心 (16)
| 文件 | 行数 | 功能 |
|------|------|------|
| `font/main.zig` | ~80 | 包根，导出所有类型 |
| `font/backend.zig` | ~100 | 后端枚举（coretext/freetype/web_canvas） |
| `font/face.zig` | ~100 | Face 类型抽象 |
| `font/library.zig` | ~50 | 字体库状态 |
| `font/shape.zig` | ~100 | Shaper 类型抽象 |
| `font/discovery.zig` | ~200 | 字体发现（fontconfig/CoreText） |
| `font/Collection.zig` | ~500 | 字体集合（按样式排序） |
| `font/DeferredFace.zig` | ~200 | 延迟加载字体面 |
| `font/CodepointMap.zig` | ~200 | 码点到字体映射 |
| `font/CodepointResolver.zig` | ~300 | 码点解析（含回退发现） |
| `font/SharedGrid.zig` | ~500 | 共享字体渲染状态 |
| `font/SharedGridSet.zig` | ~200 | 共享字体网格集合 |
| `font/Atlas.zig` | ~400 | 纹理图集（矩形装箱） |
| `font/Glyph.zig` | ~50 | 加载的字形 |
| `font/Metrics.zig` | ~200 | 字体度量 |
| `font/sprite.zig` | ~100 | 精灵字体枚举 |
| `font/opentype.zig` | ~100 | OpenType 解析入口 |
| `font/nerd_font_attributes.zig` | ~300 | Nerd Font 缩放约束 |
| `font/embedded.zig` | ~100 | 内嵌 JetBrains Mono 字体 |

### 字体面后端 (3)
| 文件 | 功能 |
|------|------|
| `font/face/coretext.zig` | macOS CoreText 字体面 |
| `font/face/freetype.zig` | FreeType 字体面 |
| `font/face/web_canvas.zig` | Web Canvas 字体面 |

### 文本整形后端 (6)
| 文件 | 功能 |
|------|------|
| `font/shaper/coretext.zig` | CoreText 整形 |
| `font/shaper/harfbuzz.zig` | HarfBuzz 整形 |
| `font/shaper/noop.zig` | 无整形 |
| `font/shaper/web_canvas.zig` | Web Canvas 整形 |
| `font/shaper/Cache.zig` | 整形结果缓存 |
| `font/shaper/run.zig` | 文本运行 |

### OpenType 解析 (6)
| 文件 | 功能 |
|------|------|
| `font/opentype/sfnt.zig` | SFNT 容器 |
| `font/opentype/head.zig` | Font Header 表 |
| `font/opentype/hhea.zig` | Horizontal Header 表 |
| `font/opentype/os2.zig` | OS/2 表 |
| `font/opentype/post.zig` | PostScript 表 |
| `font/opentype/svg.zig` | SVG 表 |

### 精灵字体 (12)
| 文件 | 功能 |
|------|------|
| `font/sprite/Face.zig` | 精灵字体面 |
| `font/sprite/canvas.zig` | 2D 绘图画布 |
| `font/sprite/draw/box.zig` | Box Drawing (U+2500-U+257F) |
| `font/sprite/draw/block.zig` | Block Elements (U+2580-U+259F) |
| `font/sprite/draw/braille.zig` | Braille Patterns (U+2800-U+28FF) |
| `font/sprite/draw/branch.zig` | Branch Drawing (git 图形) |
| `font/sprite/draw/geometric_shapes.zig` | Geometric Shapes |
| `font/sprite/draw/powerline.zig` | Powerline 符号 |
| `font/sprite/draw/special.zig` | 光标、下划线等特殊精灵 |
| `font/sprite/draw/common.zig` | 共享绘制工具 |
| `font/sprite/draw/symbols_for_legacy_computing.zig` | Legacy Computing |
| `font/sprite/draw/symbols_for_legacy_computing_supplement.zig` | Legacy Computing 补充 |

## 核心架构

### 1. 字体集合 (`Collection.zig`)

```
Collection:
  ├── faces: [4]?Face        - regular/bold/italic/bold_italic
  ├── deferred: [4]?DeferredFace - 延迟加载引用
  ├── metrics: Metrics        - 统一度量
  ├── codepoint_map: CodepointMap - 码点覆盖映射
  └── 方法:
        ├── load()            - 加载字体
        ├── glyph()           - 获取字形
        ├── hasCodepoint()    - 检查码点覆盖
        └── resize()          - 调整大小
```

### 2. 码点解析 (`CodepointResolver.zig`)

```
解析链:
1. CodepointMap 精确匹配
2. Collection.faces 按优先级查找
3. 精灵字体 (sprite glyphs)
4. 字体发现回退 (discovery fallback)
5. 描述符缓存 (descriptor cache)
```

### 3. 文本整形 (`shaper/`)

```
Shaper:
  ├── shape(text, options) → []Glyph
  │     ├── 运行分段 (相同字体+样式的连续字符)
  │     ├── 整形引擎处理 (HarfBuzz/CoreText)
  │     └── 返回字形位置数组
  └── Cache: HashMap(run_hash, []Glyph)
```

### 4. 纹理图集 (`Atlas.zig`)

```
Atlas:
  ├── format: enum { grayscale, bgr, bgra }
  ├── width, height: u32
  ├── nodes: []Node (skyline 数据结构)
  ├── data: []u8 (像素数据)
  └── 方法:
        ├── alloc(w, h) → ?Region  - 分配区域
        ├── resize()               - 扩大图集
        └── upload()               - 上传到 GPU
```

### 5. 共享字体网格 (`SharedGrid.zig`)

```
SharedGrid:
  ├── atlas_grayscale: Atlas   - 灰度字形图集
  ├── atlas_color: Atlas       - 彩色字形图集
  ├── glyph_cache: HashMap     - 码点 → Glyph 查找
  ├── codepoint_resolver: CodepointResolver
  ├── metrics: Metrics         - 单元格尺寸
  ├── rwlock: RwLock           - 读写锁（多表面共享）
  └── font_size: f32
```

### 6. 字体度量 (`Metrics.zig`)

```
Metrics:
  ├── cell_width, cell_height: u32
  ├── baseline: i32
  ├── underline: { position, thickness }
  ├── strikethrough: { position, thickness }
  ├── overline: { position, thickness }
  ├── box_thickness: u32
  ├── cursor: { x, height, bar_width }
  └── face: FaceMetrics (ascent, descent, line_gap)
```

### 7. 精灵字体

为以下特殊字符提供纯 CPU 绘制的字形：
- Box Drawing (U+2500-U+257F): ─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼
- Block Elements (U+2580-U+259F): ▀ ▄ █ ▌ ▐ ░ ▒ ▓
- Braille Patterns (U+2800-U+28FF): ⠀ ⠁ ⠂ ⠃ ...
- Powerline:      
- 光标样式: █ ▏ ▔
- 下划线/删除线/上划线

## 纯 Rust 实现要点

1. **字体发现**：macOS 用 `core-text` crate，Linux 用 `fontconfig` crate
2. **光栅化**：用 `rustybuzz` + FreeType 或 `ab_glyph`
3. **文本整形**：`rustybuzz` (HarfBuzz Rust 绑定)
4. **纹理图集**：`guillotière` crate
5. **OpenType 解析**：`ttf-parser` 或 `allsorts`
6. **精灵字体**：纯 CPU 绘制，用 Rust 实现 2D 绘图

### 推荐 crate 组合

```toml
[dependencies]
ab-glyph = "0.2"         # 字体加载 + 光栅化
rustybuzz = "0.18"       # HarfBuzz 文本整形
guillotiere = "0.6"      # 矩形装箱图集
ttf-parser = "0.24"      # OpenType 解析
core-text = "20"         # macOS CoreText
fontconfig = "0.9"       # Linux fontconfig
```
