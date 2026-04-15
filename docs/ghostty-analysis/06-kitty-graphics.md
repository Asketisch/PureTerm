# 06 - Kitty 图形协议

## 功能概述

完整的 Kitty 图形协议实现，支持在终端中显示和操作图像。包括图像传输、渲染放置、文件加载、Unicode 占位符放置等。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `terminal/kitty/graphics.zig` | ~100 | 命名空间，重新导出子模块 |
| `terminal/kitty/graphics_command.zig` | ~600 | 命令解析器 (APC G 解析) |
| `terminal/kitty/graphics_exec.zig` | ~500 | 命令执行（图像加载/放置/删除） |
| `terminal/kitty/graphics_image.zig` | ~500 | 图像加载和存储 (PNG 解码、分块传输) |
| `terminal/kitty/graphics_render.zig` | ~200 | 渲染放置描述（位置、尺寸） |
| `terminal/kitty/graphics_storage.zig` | ~400 | 每屏幕图像存储（LRU 淘汰、放置管理） |
| `terminal/kitty/graphics_unicode.zig` | ~200 | Unicode 占位符虚拟放置 |
| `terminal/kitty/key.zig` | ~100 | Kitty 键盘协议标志 |
| `terminal/kitty/color.zig` | ~150 | Kitty 颜色协议类型 |

## 核心数据结构

### 1. Kitty 图形命令 (`graphics_command.zig`)

```
格式: ESC _ G <control_keys>=<base64_data> ESC \

Command (union enum):
  ├── transmit: Transmit     - 传输图像数据
  ├── display: Display       - 显示/放置图像
  ├── query: Query           - 查询图像信息
  ├── delete: Delete         - 删除图像
  ├── framing: Framing       - 分块传输控制
  ├── transmit_animated: TransmitAnimated - 传输动画帧
  └── place: Place           - 放置已加载图像
```

**Transmit 参数**:
```
  a=transmit
  f=<format>        - 32(RGBA), 24(RGB), 100(PNG), ...
  s=<width>,v=<height>  - 图像尺寸 (像素)
  S=<size>          - 数据大小 (压缩前)
  O=<offset>        - 分块偏移
  i=<image_id>      - 图像 ID
  I=<placement_id>  - 放置 ID
  c=<cols>,r=<rows> - 占用的网格单元数
  m=1               - 更多数据跟随
  z=<z_index>       - Z 轴顺序
  t=<data>          - 压缩数据
```

**Display/Delete 参数**:
```
  a=display/delete
  i=<image_id>      - 目标图像
  I=<placement_id>  - 目标放置
  c=<cols>,r=<rows> - 放置尺寸
  x=<px>,y=<py>     - 像素偏移
  w=<pw>,h=<ph>     - 源矩形
  z=<z_index>       - Z 轴顺序
  C=<crop>          - 裁剪
  p=<placement>     - 放置策略
```

### 2. 图像加载 (`graphics_image.zig`)

```
LoadingImage:
  ├── 方式:
  │     ├── direct       - 直接数据传输
  │     ├── file         - 文件路径加载
  │     └── tempfile     - 临时文件
  ├── 格式支持:
  │     ├── PNG          - 自动解码
  │     ├── RGB/RGBA     - 原始像素
  │     └── 压缩数据     - zlib 解压
  └── 分块传输:
        ├── 追加数据块
        ├── 检查完整性
        └── 最终组装

Image:
  ├── id: u32           - 图像 ID
  ├── width, height: u32 - 像素尺寸
  ├── data: []u8        - RGBA 像素数据
  └── format: PixelFormat
```

### 3. 图像存储 (`graphics_storage.zig`)

```
ImageStorage:
  ├── images: HashMap(u32, Image)    - ID → 图像
  ├── placements: HashMap(PlacementKey, Placement) - 放置
  ├── dirty: bool                    - 脏标记
  ├── byte_limit: u32                - 字节限制 (默认 320MB)
  └── 方法:
        ├── put()                    - 存储图像
        ├── get()                    - 获取图像
        ├── place()                  - 添加放置
        ├── remove()                 - 移除图像/放置
        ├── evict()                  - LRU 淘汰超限图像
        └── updateFromPageList()     - 同步页面生命周期
```

### 4. 渲染放置 (`graphics_render.zig`)

```
Placement:
  ├── image_id: u32
  ├── placement_id: u32
  ├── x, y: u16         - 网格坐标
  ├── z: i16            - Z 轴顺序
  ├── width, height: u16 - 网格尺寸
  ├── pixel_offset_x/y: i16 - 像素偏移
  ├── source_rect: ?Rect - 源矩形裁剪
  └── dest_size: ?Size   - 目标尺寸
```

### 5. Unicode 占位符 (`graphics_unicode.zig`)

使用 Unicode 私有区域 U+10EEEE 作为占位符：
- 单元格中存储占位符 codepoint
- 放置信息编码在单元格元数据中
- 支持跨行、跨列的虚拟放置

## Kitty 键盘协议 (`key.zig`)

```
Flags (packed struct):
  ├── disambiguate: bool       - 区分修饰键组合
  ├── report_events: bool      - 报告按键事件
  ├── report_alternates: bool  - 报告替代按键
  ├── report_all: bool         - 报告所有按键
  └── report_associated: bool  - 报告关联文本
```

## 纯 Rust 实现要点

1. **命令解析**：解析 `key=value` 对，处理 base64 数据
2. **图像存储用 `HashMap<u32, Image>`**：带 LRU 淘汰
3. **PNG 解码用 `png` crate**：或 Wuffs（如果需要高性能）
4. **分块传输用状态机**：记录当前偏移和已接收数据
5. **放置管理**：跟踪每个放置的网格位置和 Z 轴顺序
6. **渲染集成**：将放置信息传递给渲染器
7. **内存限制**：实现字节限制和自动淘汰

### 实现优先级

Kitty 图形协议属于 P2 级别，可以在核心终端功能完成后实现。但 `kitty/key.zig` 的键盘协议是 P0，因为影响键盘编码。

```rust
pub struct KittyImageStorage {
    images: HashMap<u32, KittyImage>,
    placements: HashMap<(u32, u32), KittyPlacement>,
    byte_limit: usize,
    used_bytes: usize,
}

pub struct KittyImage {
    id: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>, // RGBA
}

pub struct KittyPlacement {
    image_id: u32,
    placement_id: u32,
    x: u16, y: u16,
    z: i16,
    width: u16, height: u16,
    // ...
}
```
