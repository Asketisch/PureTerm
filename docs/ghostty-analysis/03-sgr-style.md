# 03 - SGR 与样式系统

## 功能概述

终端文本的视觉属性管理：颜色（前景/背景/下划线）、粗体、斜体、下划线、删除线等。包括 256 色调色板、RGB 颜色、X11 颜色名称解析。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `terminal/sgr.zig` | ~800 | SGR 参数解析器 |
| `terminal/style.zig` | ~400 | Style 结构体定义 |
| `terminal/color.zig` | ~600 | 256 色调色板、颜色插值 |
| `terminal/x11_color.zig` | ~2000 | X11 颜色名称查找表（comptime 生成） |
| `terminal/ref_counted_set.zig` | ~300 | 引用计数样式去重集 |

## 核心数据结构

### 1. SGR 属性 (`sgr.zig`)

SGR (Select Graphic Rendition) 参数解析器的输出：

```
Attribute (union enum):
  ├── reset                  - 重置所有属性
  ├── bold                   - 粗体
  ├── dim                    - 暗淡
  ├── italic                 - 斜体
  ├── underline: Underline   - 下划线样式
  │     ├── off
  │     ├── single
  │     ├── double
  │     ├── curly
  │     ├── dotted
  │     └── dashed
  ├── blink                  - 闪烁
  ├── reverse                - 反色
  ├── hidden                 - 隐藏
  ├── strikethrough          - 删除线
  ├── overline               - 上划线
  ├── foreground: Color      - 前景色
  ├── background: Color      - 背景色
  ├── foreground_bright: u8  - 明亮前景色索引 (8-15)
  ├── background_bright: u8  - 明亮背景色索引 (8-15)
  └── reset_foreground/background - 重置颜色
```

颜色类型 (`Color` union)：
```
Color:
  ├── default                - 默认颜色
  ├── palette: u8            - 调色板索引 (0-255)
  └── rgb: [3]u8             - RGB 值
```

### 2. Style (`style.zig`)

单元格的完整视觉属性：

```
Style (packed, 32-bit):
  ├── fg: Color              - 前景色
  ├── bg: Color              - 背景色
  ├── underline: Color       - 下划线颜色
  ├── flags:
  │     ├── bold: bool
  │     ├── dim: bool
  │     ├── italic: bool
  │     ├── blink: bool
  │     ├── inverse: bool
  │     ├── invisible: bool
  │     ├── strikethrough: bool
  │     ├── overline: bool
  │     └── underline: Underline (3 bits)
  └── 通过 RefCountedSet 去重存储
```

**关键设计**：Style 通过 RefCountedSet 存储和去重。当设置新样式时，旧的引用计数减少，新的引用计数增加。这意味着数百万个单元格可以共享少数几种样式。

### 3. 256 色调色板 (`color.zig`)

```
Palette:
  ├── colors: [256]RGB       - 256 色数组
  ├── mask: Mask             - 哪些颜色被修改过
  └── 生成逻辑:
        ├── 0-7: 标准 ANSI 颜色
        ├── 8-15: 明亮变体
        ├── 16-231: 6x6x6 色彩立方体
        └── 232-255: 24 级灰度
```

**三线性 CIELAB 插值**：从 base16 主题的 8 种颜色生成完整的 256 色调色板，使用 CIELAB 色彩空间进行平滑插值。

### 4. X11 颜色名称 (`x11_color.zig`)

comptime 从 `rgb.txt` 解析的颜色名称查找表：
- ~750 个颜色名称
- 大小写不敏感
- 返回 RGB 值

### 5. 引用计数集 (`ref_counted_set.zig`)

```
RefCountedSet(T):
  ├── open addressing + linear probing
  ├── Robin Hood hashing
  ├── 引用计数: insert增加, remove减少
  └── 零引用时自动移除
```

## SGR 解析逻辑

SGR 序列 `CSI Pm m` 的解析规则：

1. **空参数** (CSI m) = 重置 (等同于 CSI 0 m)
2. **标准属性**：0=重置, 1=粗体, 2=暗淡, 3=斜体, 4=下划线, 5=闪烁, 7=反色, 8=隐藏, 9=删除线, 53=上划线
3. **下划线变体**：4:0=关, 4:1=单线, 4:2=双线, 4:3=波浪线, 4:4=点线, 4:5=虚线
4. **前景色**：30-37(标准), 38(扩展), 90-97(明亮)
5. **背景色**：40-47(标准), 48(扩展), 100-107(明亮)
6. **扩展色格式**：
   - `38;5;n` - 256 色索引
   - `38;2;r;g;b` - RGB 真彩色
7. **重置颜色**：39=前景默认, 49=背景默认

## 纯 Rust 实现要点

1. **SGR 解析**：简单的状态机，逐个参数分发
2. **Style 结构**：用 bitflags crate 或手写 bitflags
3. **颜色用 `enum { Default, Palette(u8), Rgb([u8; 3]) }`**
4. **RefCountedSet**：用 `HashMap<Style, u32>` 实现引用计数
5. **256 色调色板生成**：三线性 CIELAB 插值需要实现 CIELAB 色彩空间转换
6. **X11 颜色**：可以用 `phf` crate 编译时生成查找表

### 关键 Rust 类型定义

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    kind: ColorKind,
}

enum ColorKind {
    Default,
    Palette(u8),
    Rgb(u8, u8, u8),
}

bitflags::bitflags! {
    pub struct TextFlags: u16 {
        const BOLD = 0x01;
        const DIM = 0x02;
        const ITALIC = 0x04;
        const BLINK = 0x08;
        const INVERSE = 0x10;
        const INVISIBLE = 0x20;
        const STRIKETHROUGH = 0x40;
        const OVERLINE = 0x80;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub underline_color: Color,
    pub flags: TextFlags,
    pub underline: Underline,
}
```
