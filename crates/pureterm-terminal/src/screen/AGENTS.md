# pureterm-terminal/src/screen — 字符网格与选择区域

终端屏幕的底层实现：字符网格管理（含 reflow 回流）、单元格数据、光标管理、文本选择与鼠标事件编码。

## 文件结构

```
src/
├── mod.rs        # 模块重导出 (13 行)
├── cell.rs       # Cell、CellAttrs、Color、NamedColor (103 行)
├── color.rs      # 重导出 Color (1 行)
├── cursor.rs     # Cursor、CursorStyle (55 行)
├── grid.rs       # Grid（字符网格 + 回滚缓冲区）(368 行)
└── selection.rs  # 文本选择 + 鼠标事件编码 (142 行)
```

**注意：** 无 `history.rs` 文件，回滚历史直接由 `Grid` 管理。

## 模块依赖

```
screen/ → hyperlink/（Cell 中的超链接）
```

无外部 crate 依赖。

## 核心类型

### Grid（grid.rs）

管理终端字符网格，包括可见区域和回滚历史。

**字段：**

| 字段 | 类型 | 说明 |
|------|------|------|
| `rows` | `Vec<Vec<Cell>>` | 所有行（可见 + 回滚） |
| `marks` | `Vec<Option<Mark>>` | 每行标记（与 rows 同索引） |
| `wrapped` | `Vec<bool>` | 每行是否被软换行（reflow 用） |
| `cols` | `usize` | 列数 |
| `visible_rows` | `usize` | 可见行数 |
| `scrollback_limit` | `usize` | 最大回滚行数 |
| `scroll_offset` | `usize` | 视口偏移（0 = 底部/最新） |

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new(cols, rows, scrollback)` | 创建网格 |
| `get(row, col) / get_mut(row, col)` | 按可见坐标访问 |
| `scroll_up() / scroll_down()` | 全屏滚动（视口不动，网格内容动） |
| `scroll_region_up(top, bottom) / scroll_region_down(top, bottom)` | 滚动区域滚动 |
| `scroll_viewport_up/down(lines)` | 在历史中滚动视口 |
| `resize(new_cols, new_rows)` | 调整尺寸，触发 **reflow** |
| `set_wrapped(row, value) / is_wrapped(row)` | 行包裹标志操作 |
| `set_mark(row, mark) / get_mark(row)` | 行标记操作 |
| `visible_cells()` / `visible_row(row)` | 获取可见行数据 |
| `history()` | 返回回滚行数 |

### Grid Reflow 机制

`resize()` 时触发 reflow（重组合并 wrapped 行，重排到新宽度）：

1. 根据 `wrapped` 标志重组合并连续软换行行为逻辑行
2. 按 `new_cols` 重新分块
3. 恢复 marks 到首行
4. 同步更新 `rows`、`marks`、`wrapped` 三个向量

**重要：** `wrapped` 向量与 `rows` 和 `marks` 必须同步维护。任何修改行的操作都要同时更新三者。

### Cell（cell.rs）

单个终端字符单元格。

**字段：**

| 字段 | 类型 | 说明 |
|------|------|------|
| `c` | `char` | 字符 |
| `fg` | `Color` | 前景色 |
| `bg` | `Color` | 背景色 |
| `attrs` | `CellAttrs` | 文本属性 |
| `hyperlink` | `Option<Arc<Hyperlink>>` | OSC 8 超链接 |

### CellAttrs（cell.rs）

字符属性（位标志风格的 struct）：

| 字段 | 类型 | 说明 |
|------|------|------|
| `bold` | `bool` | 粗体（→ palette 0-7 亮色映射） |
| `dim` | `bool` | 暗淡 |
| `italic` | `bool` | 斜体 |
| `underline` | `bool` | 下划线 |
| `strikethrough` | `bool` | 删除线 |
| `inverse` | `bool` | 反色 |
| `hidden` | `bool` | 隐藏文本 |
| `double_width` | `bool` | 双宽字符（右半部分为占位） |

### Color（cell.rs）

颜色类型：

```rust
pub enum Color {
    Named(NamedColor),   // 命名颜色（Foreground、Background 等）
    Palette(u8),         // 调色板索引 (0-255)
    Rgb(u8, u8, u8),    // RGB 真彩色
}
```

### NamedColor（cell.rs）

命名颜色枚举：Foreground、Background、Cursor、SelectionBackground、SelectionForeground 以及 16 色调色板（Black、Red、Green、Yellow、Blue、Magenta、Cyan、White + Bright 版本）。

### Cursor（cursor.rs）

终端光标状态。

**字段：**

| 字段 | 类型 | 说明 |
|------|------|------|
| `row` / `col` | `usize` | 光标位置 |
| `style` | `CursorStyle` | 光标样式 |
| `blink` | `bool` | 闪烁状态 |
| `visible` | `bool` | 可见性 |
| `saved_row` / `saved_col` | `usize` | DECRC 保存的位置 |

**方法：**
- `save()` — 将 `row/col` 复制到 `saved_row/saved_col`
- `restore()` — 将 `saved_row/saved_col` 复制回 `row/col`

### CursorStyle（cursor.rs）

```rust
pub enum CursorStyle {
    Block,    // 方块光标
    Underline,// 下划线光标
    Beam,     // 光束光标（I 形）
}
```

## 选择区域与鼠标事件（selection.rs）

### 单词选择

- `is_word_char(c)` — 判断是否为单词字符（`a-zA-Z0-9_-.@/`）
- `find_word_bounds(line, col)` — 查找单词边界（双击选词用）

### 鼠标事件类型

```rust
pub enum MouseButton { Left, Middle, Right }
pub enum MouseModifiers { ctrl, alt, shift }
pub enum ClickCount { Single, Double, Triple }

pub struct TerminalMouseEvent {
    pub kind: MouseEventKind, // Down/Up/Move/Scroll
    pub button: MouseButton,
    pub modifiers: MouseModifiers,
    pub click_count: ClickCount,
    pub row: usize,
    pub col: usize,
}
```

### 鼠标编码格式

- `encode_mouse_x10(event)` — X10 格式（仅 8 色，1.1 协议）
- `encode_mouse_sgr(event)` — SGR 格式（256 色，1006 扩展协议）

SGR 格式：`\x1b[<Pb;Px;PwM`（Down）、`\x1b[<Pb;Px;PwM`（Up）、`\x1b[<Pb;Px;Pw;PcM`（Move，Pc=3）

## 父级导航

← [pureterm-terminal/AGENTS.md](../AGENTS.md)
