# pureterm-terminal/src/parser — VTE 执行引擎

VTE 转义序列的解析与执行核心模块。`perform.rs` 实现 `vte::Perform` trait，约 1263 行，是终端模拟引擎中最复杂的文件。

## 文件结构

```
src/
├── mod.rs       # VteParser 包装 (27 行)
└── perform.rs   # VtePerform（vte::Perform 实现）+ ScreenSnapshot (1263 行)
```

## 模块依赖

```
parser/ → screen/（Grid、Cell、Cursor、TermMode）
```

无外部 crate 依赖（vte crate 由父级引入）。

## 核心类型

### VtePerform（perform.rs）

`VtePerform` 实现 `vte::Perform` trait，是 VTE 解析器的回调执行器。

**字段：**

| 字段 | 类型 | 说明 |
|------|------|------|
| `grid` | `Rc<RefCell<Grid>>` | 字符网格（含可见行 + 回滚缓冲区） |
| `cursor` | `Cursor` | 光标位置、样式、闪烁状态 |
| `mode` | `TermMode` | 当前终端模式标志（u32 位标志） |
| `fg` / `bg` | `Color` | 当前前景/背景色 |
| `attrs` | `CellAttrs` | 当前字符属性（bold、italic 等） |
| `title` | `String` | 终端标题 |
| `current_dir` | `String` | 当前工作目录 |
| `dirty` | `bool` | 脏标记（需重新渲染） |
| `cols` / `rows` | `usize` | 终端列/行数 |
| `saved_cursor` | `Cursor` | DECSC 保存的光标 |
| `saved_attrs` | `CellAttrs` | DECSC 保存的属性 |
| `scroll_top` / `scroll_bottom` | `usize` | 滚动区域上下边界 |
| `tab_stops` | `Vec<usize>` | Tab 停止位置 |
| `pending_wrap` | `bool` | 等待软换行（下一个字符在行尾） |
| `response_writer` | `Option<Box<dyn Write + Send>>` | DSR 等响应输出目标 |
| `alt_buffer` | `Option<AltBufferData>` | 备用屏幕缓冲区（Alt Screen） |
| `dcs_buffer` | `DcsBuffer` | DCS 序列累积缓冲区 |
| `intermediates` | `Vec<u8>` | CSI/DCS 中间字节 |
| `last_action` | `CsiAction` | 最近 CSI action（用于参数解析） |
| `last_char` | `char` | 最近打印字符（用于 REP 重复） |
| `bell_triggered` | `bool` | Bell 事件触发标志 |
| ` hyperlinks` | `Vec<Hyperlink>` | OSC 8 超链接池 |

### AltBufferData

备用屏幕缓冲区数据结构：

| 字段 | 类型 | 说明 |
|------|------|------|
| `grid` | `Grid` | 完整的备用网格 |
| `cursor` | `Cursor` | 备用光标 |
| `scroll_top` / `scroll_bottom` | `usize` | 备用滚动区域 |
| `saved_cursor` / `saved_attrs` | Cursor / CellAttrs | 备用保存状态 |

### ScreenSnapshot

渲染用的屏幕快照（由 `Terminal::snapshot()` 返回）：

| 字段 | 类型 | 说明 |
|------|------|------|
| `rows` | `Vec<Vec<Cell>>` | 可见行数据（已清理超链接） |
| `cursor_row` / `cursor_col` | `usize` | 光标位置 |
| `cursor_visible` | `bool` | 光标可见性 |
| `cursor_style` | `CursorStyle` | 光标样式（Block/Underline/Beam） |

## vte::Perform Trait 方法

### print(c: char)

打印字符到当前光标位置：
1. 存入 `last_char`（用于 REP）
2. 调用 `put_char_at_cursor(c)`，包含：
   - pending_wrap 处理（设置 wrapped flag）
   - IRM 插入模式字符右移
   - 双宽字符占位
   - 光标列递增
   - 设置 pending_wrap

### execute(byte: u8)

执行控制字符：

| 字节 | 名称 | 行为 |
|------|------|------|
| 0x07 | BEL | 设 `bell_triggered = true` |
| 0x08 | BS | 清 `pending_wrap` |
| 0x09 | HT | 移到下一 Tab 停止位 |
| 0x0A | LF | 换行（含滚动区域处理），清 `pending_wrap` |
| 0x0D | CR | 回车，清 `pending_wrap` |

### csi_dispatch(params: &[i64], intermediates: &[u8], action: char)

CSI 序列分发。`action` 是最终字符，决定操作类型。

**CSI 指令表：**

| 字母 | 名称 | 说明 |
|------|------|------|
| A | CUU | 光标上移，**清 pending_wrap** |
| B | CUD | 光标下移，**清 pending_wrap** |
| C | CUF | 光标右移，**清 pending_wrap** |
| D | CUB | 光标左移，**清 pending_wrap** |
| E | CNL | 下一行开头，**清 pending_wrap** |
| F | CPL | 上一行开头，**清 pending_wrap** |
| G | CHA | 水平绝对定位（列），**清 pending_wrap** |
| H | CUP | 光标定位（行，列），**清 pending_wrap** |
| d | VPA | 垂直绝对定位（行），**清 pending_wrap** |
| f | HVP | 同 CUP |
| J | ED | 擦除显示（0=到末尾，1=到开头，2=全部，3=保存） |
| K | EL | 擦除行（0=到末尾，1=到开头，2=全部） |
| L | IL | 插入行（滚动区域内） |
| M | DL | 删除行（滚动区域内） |
| P | DCH | 删除字符（光标处右移） |
| @ | ICH | 插入字符（光标处右移） |
| S | SU | 向上滚动（整个滚动区域） |
| T | SD | 向下滚动（整个滚动区域） |
| X | ECH | 擦除字符（不移动光标） |
| c | DA | 设备属性：`?` 前缀 → DA2，`>` 前缀 → DA2，无前缀 → DA1 |
| d | VPA | 垂直绝对定位 |
| g | TBC | 清除 Tab（0=当前位置，3=全部） |
| h | SM | 设置模式（DEC 私有 + ANSI） |
| l | RM | 重置模式（DEC 私有 + ANSI） |
| m | SGR | 文本属性：bold/dim/italic/underline/inverse/hidden/strikethrough + 颜色 |
| n | DSR | 设备状态报告（5=OK，6=CPR） |
| p | DECSTR | `CSI ! p` 软终端复位 |
| r | DECSTBM | 设置滚动区域 |
| s | SCP | 保存光标位置 |
| t | XTWINOPS | 窗口操作（18=报告文本区域大小） |
| u | RCP | 恢复光标位置 |
| I | CHT | 前进 Tab，**清 pending_wrap** |
| Z | CBT | 后退 Tab，**清 pending_wrap** |
| q | DECSCUSR | 设置光标样式（Block/Underline/Beam ± blink） |
| b | REP | 重复前一字符（使用 `last_char`） |

**模式处理（SM/RM）：**

DECSET/DECRST 私有模式：
- `1` — App Cursor Key (DECCKM)
- `3` — Column 132 (DECCOLM)
- `5` — Inverse Video (DECSCNM)
- `6` — Origin Mode (DECOM)
- `7` — Auto Wrap (DECAWM)
- `12` — Cursor Blink (att610)
- `25` — Show Cursor (DECTCEM)
- `1000/1002/1003/1006` — Mouse Reporting
- `1004` — Focus Reporting (DECFKM)
- `1047/1048/1049` — Alternate Screen Buffer
- `2004` — Bracketed Paste

ANSI SM/RM：
- `4` — Insert Mode (IRM)
- `20` — Line Feed New Line (LNM)

### osc_dispatch(command: i64, params: &[&str])

OSC 序列分发（标题、颜色、剪贴板等）：

| command | 名称 | 说明 |
|---------|------|------|
| 0 | SetTitle | 设置窗口标题（参数 0） |
| 1 | SetCurrentDir | 设置当前目录（参数 0） |
| 2 | ResetTitle | 重置标题 |
| 4 | SetColors | 设置调色板颜色 |
| 7 | CurrentDir | 当前目录查询 |
| 8 | SetHyperlink | 设置超链接 |
| 10 | SetFgColor | 设置前景色 |
| 11 | SetBgColor | 设置背景色 |
| 12 | SetCursorColor | 设置光标颜色 |
| 52 | Clipboard | 剪贴板读写 |
| 133 | ShellIntegration | Shell 集成标记 |

### esc_dispatch(intermediates: &[u8], action: char)

ESC 序列分发：

| action | 名称 | 说明 |
|--------|------|------|
| 7 | DECSC | 保存光标位置和属性 |
| 8 | DECRC | 恢复光标位置和属性 |
| c | RIS | 硬复位（清屏、重置模式） |
| D | IND | 索引（下一行，如需则滚动） |
| M | RI | 反向索引（上一行，如需则反向滚动） |
| E | NEL | 下一行（换行 + 光标移到行首） |
| H | HTS | 设置 Tab 停止位 |
| # | Alignment | 屏幕对齐测试（填充显示） |

### hook / put / unhook

DCS（Device Control String）序列处理：
- `hook` — 开始累积 DCS 数据到 `dcs_buffer`
- `put` — 追加字节到 DCS 缓冲区
- `unhook` — 结束 DCS 序列

目前主要用于 Sixel/Kitty 图像协议的日志记录（未完整实现）。

## 关键行为详解

### pending_wrap 机制

软换行标志，控制下一字符是否从新行开始：

- `put_char_at_cursor()` 在写字符前检查 `pending_wrap`，如为 true 则：
  1. 调用 `grid.set_wrapped(cursor.row, true)`
  2. 光标移到下一行（如需滚动则滚动）
  3. 清 `pending_wrap = false`

- **所有光标移动命令都清除 pending_wrap**：CUU/CUD/CUF/CUB/CUP/CHA/VPA/BS/CR

### 双宽字符处理

当 `print(c)` 遇到双宽字符时：
1. 调用 `put_char_at_cursor(c)` 写入主字符
2. 写入占位单元格（双宽右半部分）
3. 光标再右移一格（跳过一个单元格）

### IRM（Insert Mode）

当 `TermMode::INSERT` 置位时，`put_char_at_cursor()` 会先将光标处到行尾的字符右移一位，再写入新字符。

### 备用屏幕缓冲区（Alt Screen）

- `AltBufferData` 保存完整 Grid + Cursor + 滚动区域
- DECSET 1047/1049 切换到备用缓冲区
- DECRST 1047 回到主缓冲区（1049 同时清屏）
- resize 时同步更新 alt_buffer 的行列数

### REP（Repeat）

`b` action（REP）重复前一字符 `last_char`：
- 参数指定重复次数（默认 1）
- 仅当 `last_char` 是可打印字符时有效

## 调试指南

### 调试字符显示问题

1. 检查 `print()` 中的 unicode 宽度处理和 `last_char` 跟踪
2. 检查 `pending_wrap` 是否在所有光标移动中被清除
3. 检查 IRM (INSERT mode) 在 `put_char_at_cursor` 中的字符右移逻辑
4. 验证双宽字符的占位处理

### 调试光标定位问题

1. 检查 `pending_wrap` 标志 — 任何光标移动命令都应清除它
2. 检查 DECRC 是否使用 `saved.row/col` 而非 `saved.saved_row/saved_col`
3. 验证 DECSC 同时保存 `cursor.clone()` 到 `saved_cursor` 和调用 `cursor.save()`

### Bell 事件处理

UI 层通过 `terminal.take_bell()` 轮询检查 bell 触发。返回 true 后标志自动清除。

## 父级导航

← [pureterm-terminal/AGENTS.md](../AGENTS.md)
