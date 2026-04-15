# 02 - 终端屏幕与网格

## 功能概述

终端屏幕的底层数据结构，管理单元格、行、页面、光标、制表位、选区等。这是终端状态的核心。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `terminal/Screen.zig` | ~5000+ | 屏幕实现（所有 VT 动作的执行者） |
| `terminal/page.zig` | ~800 | 页面数据结构（固定大小内存块） |
| `terminal/PageList.zig` | ~1500 | 双向链表页面管理 |
| `terminal/Terminal.zig` | ~1000 | 终端高层状态 |
| `terminal/point.zig` | ~400 | 坐标/点类型 |
| `terminal/size.zig` | ~300 | 大小/偏移类型 |
| `terminal/Tabstops.zig` | ~200 | 制表位管理 |
| `terminal/Selection.zig` | ~600 | 文本选区 |
| `terminal/ScreenSet.zig` | ~200 | 主/备屏幕集 |
| `terminal/bitmap_allocator.zig` | ~300 | 位图内存分配器 |
| `terminal/hash_map.zig` | ~400 | 可移动哈希表 |
| `terminal/formatter.zig` | ~800 | 终端内容格式化 |

## 核心数据结构

### 1. 页面 (`page.zig`)

每个 Page 是一个固定大小的自包含内存块：

```
Page:
  ├── cells: []Cell          - 单元格数组（row * cols）
  ├── rows: usize            - 行数
  ├── styles: RefCountedSet  - 样式去重存储
  ├── graphemes: BitmapAllocator - 字形数据（组合字符等）
  ├── hyperlinks: OffsetHashMap - 超链接映射
  └── memory: OS-aligned allocation - OS 对齐内存分配
```

**关键设计**：
- 使用偏移量（offset）而非指针，使整个页面可以移动而不失效
- 样式去重：通过 RefCountedSet 存储，多个单元格共享同一样式
- 字形数据：通过 BitmapAllocator 分配，支持变长 Unicode 字形

### 2. 单元格 (Cell)

```
Cell:
  ├── codepoint: u21         - Unicode 码点（0 = 空）
  ├── style_id: StyleId      - 样式索引（指向 RefCountedSet）
  ├── hyperlink_id: HyperlinkId - 超链接索引
  ├── wide: WideMode         - 宽字符模式（normal/wide/spacer）
  ├── content: Content       - 内容标记（codepoint/image_placeholder）
  ├── protection: Protection - 保护模式（默认/受保护）
  ├── semantic: Semantic     - 语义标记（prompt/input/output）
  └── padding: bool          - 是否为填充单元格
```

### 3. PageList (`PageList.zig`)

双向链表管理多个页面：

```
PageList:
  ├── head/tail: ?*Node      - 链表头尾
  ├── active: *Node          - 当前活动页面
  ├── viewport: Viewport     - 视口位置
  ├── cursor: Cursor         - 光标状态
  ├── scrollbar: Scrollbar   - 滚动条状态
  ├── memory_pools:          - 内存池
  │     ├── node_pool: SegmentedPool
  │     ├── page_pool: SegmentedPool
  │     └── pin_pool: SegmentedPool
  └── pins: Pin tracking     - 稳定引用追踪
```

**关键操作**：
- `scrollUp()` / `scrollDown()` - 滚动（添加/移除页面）
- `growViewport()` / `shrinkViewport()` - 视口调整
- `resize()` - 终端大小改变
- `pin()` / `unpin()` - 创建/销毁稳定引用

### 4. 屏幕 (`Screen.zig`)

实现所有 VT 动作的核心：

```
Screen:
  ├── pages: PageList        - 页面列表
  ├── cursor: Cursor         - 光标
  ├── selection: ?Selection  - 当前选区
  ├── charsets: [4]Charset   - 字符集映射 (G0-G3)
  ├── tabstops: Tabstops     - 制表位
  ├── scroll_region: ?Region - 滚动区域
  ├── kitty_images: ImageStorage - Kitty 图像存储
  ├── semantic_prompts: SemanticPromptTracker
  ├── saved_cursor: [2]Cursor - 保存的光标（主/备屏幕）
  └── modes: *Modes          - 终端模式
```

**主要 VT 动作方法**：
- `writeString()` / `writeCell()` - 写入文本
- `index()` / `reverseIndex()` - 上下移动一行（IND/RI）
- `linefeed()` - 换行
- `cursorUp/Down/Forward/Back()` - 光标移动
- `cursorPosition()` - 绝对定位
- `eraseDisplay()` / `eraseLine()` - 擦除
- `insertLines()` / `deleteLines()` - 行操作
- `insertChars()` / `deleteChars()` - 字符操作
- `scrollUp()` / `scrollDown()` - 滚动
- `setScrollRegion()` - 设置滚动区域
- `setTabstop()` / `clearTabstop()` / `clearAllTabstops()` - 制表位
- `saveCursor()` / `restoreCursor()` - 光标保存/恢复
- `switchScreen()` - 主/备屏幕切换

### 5. 终端 (`Terminal.zig`)

高层终端状态：

```
Terminal:
  ├── screen_set: ScreenSet  - 主屏幕 + 备屏幕
  ├── modes: Modes           - 所有终端模式
  ├── default_modes: Modes   - 重置时的默认模式
  ├── cursor_style: CursorStyle - 光标样式
  ├── title: ?[:0]const u8   - 窗口标题
  ├── colors: Colors         - 颜色状态
  ├── mouse: MouseState      - 鼠标状态
  ├── kitty_keyboard: Flags  - Kitty 键盘标志
  ├── dirty: DirtyFlags      - 脏标记
  └── options: Options       - 终端选项
```

### 6. 坐标系统 (`point.zig`)

四种坐标参考系：

| 类型 | 说明 | 起始点 |
|------|------|--------|
| `Active` | 光标可寻址区域 | 活动区域左上角 |
| `Viewport` | 可见区域 | 视口左上角 |
| `Screen` | 包含回滚的完整屏幕 | 屏幕最顶部 |
| `History` | 仅回滚区域 | 回滚最顶部 |

### 7. 选区 (`Selection.zig`)

```
Selection:
  ├── start/end: Pin         - 选区起止（tracked pin）
  ├── rectangle: bool        - 矩形选择模式
  └── 方法:
        ├── text()           - 获取选区文本
        ├── contains()       - 检查点是否在选区内
        ├── adjust()         - 调整选区
        └── rectangular()    - 矩形选择文本提取
```

## 纯 Rust 实现要点

1. **Page 用 `Vec<Cell>` + 固定容量**：Rust 的所有权模型让偏移量设计不那么必要，但保留偏移量语义可以方便内存重分配
2. **RefCountedSet → `Rc<Style>` 或索引池**：用引用计数或索引池做样式去重
3. **PageList → `VecDeque<Page>`**：或手写双向链表
4. **BitmapAllocator → bump allocator 或 slab allocator**
5. **Cursor → 简单结构体**：row, col, style, visibility
6. **Selection → Pin-based 追踪**：在 Rust 中可以用 index + generation 实现

### 关键实现细节

- 宽字符（CJK）需要 2 个单元格，第二个为 spacer
- Tabstop 管理用 BitSet
- 滚动区域只影响指定行范围
- 主/备屏幕各自维护独立的 PageList
- 选区使用 tracked pin 在滚动和回滚中保持有效
