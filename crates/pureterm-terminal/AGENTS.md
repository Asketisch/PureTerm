# pureterm-terminal — VTE 解析器、屏幕网格、终端模拟

终端模拟引擎的核心实现：VTE 转义序列解析、字符网格管理、光标操作。

## 关键依赖

`vte`（VTE 解析）、`unicode-width` + `unicode-segmentation`（Unicode）、`serde`（序列化）、`thiserror`（错误）、`parking_lot`（并发）。

无内部依赖（standalone crate）。

## 文件结构

```
src/
├── lib.rs           # 模块导出 + pub use
├── term.rs          # Terminal 结构体（顶层接口）
├── mode.rs          # TermMode 位标志
├── parser/
│   ├── mod.rs       # VteParser 包装
│   └── perform.rs   # VtePerform（vte::Perform 实现）+ ScreenSnapshot
└── screen/
    ├── mod.rs
    ├── cell.rs      # Cell、CellAttrs、Color、NamedColor
    ├── color.rs     # 重导出 Color
    ├── cursor.rs    # Cursor、CursorStyle
    └── grid.rs      # Grid（回滚缓冲区 + 可见视口）
```

## 数据流

```
Session 字节 → VteParser::parse() → VtePerform（vte::Perform）→ Grid/Cell → ScreenSnapshot → wgpu 渲染
```

## 模块详情

### term.rs — 顶层接口

`Terminal` 组合了 `VtePerform` + `VteParser`：

| 方法 | 说明 |
|------|------|
| `new(cols, rows, scrollback)` | 创建终端实例 |
| `write(data)` | 输入字节流，经解析器处理 |
| `resize(cols, rows)` | 调整终端尺寸 |
| `snapshot() -> ScreenSnapshot` | 获取当前可见屏幕快照 |
| `is_dirty()` / `clear_dirty()` | 脏标记管理 |
| `title()` / `current_dir()` | 终端标题和当前目录 |
| `scroll_up/down(lines)` / `scroll_to_bottom()` | 视口滚动 |
| `get_text(start_row, start_col, end_row, end_col)` | 提取文本（用于复制） |
| `get_visible_text()` | 获取全部可见文本 |

### mode.rs — 终端模式标志

`TermMode`（u32 位标志）：`SHOW_CURSOR`、`APP_CURSOR`、`APP_KEYPAD`、`AUTO_WRAP`、`LINE_FEED_NEW_LINE`、`ORIGIN`、`COLUMN_132`、`INVERSE`、`VISIBLE_BELL`、`MOUSE_REPORT_CLICK`、`MOUSE_DRAG`、`MOUSE_MOTION`、`MOUSE_SGR`、`BRACKETED_PASTE`、`FOCUS_IN_OUT`。

### screen/cell.rs — 单元格

| 类型 | 说明 |
|------|------|
| `Cell` | 单个字符单元格：`c: char`、`fg/bg: Color`、`attrs: CellAttrs` |
| `CellAttrs` | 文本属性：bold、dim、italic、underline、strikethrough、inverse、hidden、double_width |
| `Color` | `Named(NamedColor)` / `Palette(u8)` / `Rgb(u8, u8, u8)` |
| `NamedColor` | Foreground、Background、Cursor、SelectionBackground、SelectionForeground |

### screen/cursor.rs — 光标

| 类型 | 说明 |
|------|------|
| `CursorStyle` | Block、Underline、Beam |
| `Cursor` | row、col、style、blink、visible、saved_row、saved_col |

### screen/grid.rs — 字符网格

`Grid` 管理可见行 + 回滚缓冲区：

| 方法 | 说明 |
|------|------|
| `new(cols, rows, scrollback)` | 创建网格 |
| `get(row, col)` / `get_mut(row, col)` | 按可见坐标访问 |
| `scroll_up()` | 内容上滚，底部加空行 |
| `scroll_down()` | 在可见区上方插入空行 |
| `resize(new_cols, new_rows)` | 调整尺寸 |
| `scroll_viewport_up/down(lines)` | 在历史中滚动视口 |
| `visible_cells()` | 返回可见部分 `Vec<&[Cell]>` |
| `history()` | 返回回滚行数 |

### parser/perform.rs — VTE 执行器

`VtePerform` 实现 `vte::Perform` trait，是最复杂的模块：

**字段**：`grid`、`cursor`、`mode`、`fg/bg: Color`、`attrs: CellAttrs`、`title`、`current_dir`、`dirty`、`cols/rows`、`saved_cursor/attrs`、`scroll_top/scroll_bottom`、`tab_stops`、`osc_buffer`。

**vte::Perform 方法**：
- `print(c)` — 字符放置，处理 unicode 宽度、自动换行、双宽字符
- `execute(byte)` — BS(0x08)、HT(0x09)、LF(0x0A 含滚动区域)、CR(0x0D)
- `csi_dispatch` — CSI 指令分发：CUU(A)、CUD(B)、CUF(C)、CUB(D)、CUP(H/f)、ED(J)、EL(K)、IL(L)、DL(M)、DCH(P)、SU(S)、SD(T)、ECH(X)、SGR(m)、SM(h)、RM(l)、SCP(s)、RCP(u)、DECSTBM(r)、CHA(G)、VPA(d)
- `osc_dispatch` — OSC 0/2（标题）、OSC 7（当前目录）

**SGR（m）处理**：支持 bold、dim、italic、underline、inverse、hidden、strikethrough、标准 8 色、256 色、truecolor RGB、aixterm 亮色。

**模式控制**：DECSET/DECRST 处理光标模式、自动换行、原点模式、鼠标报告、括号粘贴。

### ScreenSnapshot

渲染用的屏幕快照：`rows: Vec<Vec<Cell>>`、`cursor_row/col`、`cursor_visible`、`cursor_style`。

## 常见任务

### 修改 VTE 解析行为
编辑 `parser/perform.rs`。CSI 指令在 `csi_dispatch` 方法中通过 `params[0]` 的字符匹配分发。SGR 处理在 `handle_sgr` 方法中。

### 添加新的 CSI 指令支持
在 `csi_dispatch` 中添加新的字符匹配分支。参考现有实现（如 CUP、ED）。

### 修改网格行为
编辑 `screen/grid.rs`。注意 `scroll_up` 的回滚限制逻辑和 `resize` 的列数调整。

### 调试字符显示问题
1. 检查 `print()` 中的 unicode 宽度处理
2. 检查 `AUTO_WRAP` 模式下的换行逻辑
3. 验证双宽字符的占位处理
