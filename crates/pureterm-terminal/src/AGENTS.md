# pureterm-terminal/src — 终端模拟引擎源码

终端模拟引擎的所有源文件：顶层 Terminal 接口、VTE 解析器、字符网格、输入编码、Tmux 集成，以及双向文本、超链接、图片协议等扩展模块。

← [pureterm-terminal/AGENTS.md](../AGENTS.md)

## 文件结构

```
src/
├── lib.rs              # 模块导出 + pub use (32 行)
├── term.rs             # Terminal 主结构体 — 顶层接口 (378 行)
├── mode.rs             # TermMode 位标志 — 终端模式管理 (104 行)
├── charset.rs          # 字符集映射 — G0-G3、DEC Special Graphics (121 行)
├── hyperlink.rs        # Hyperlink — OSC 8 超链接 (50 行)
├── mark.rs             # Mark / MarkKind — 语义标记（OSC 133）(104 行)
├── block.rs            # CommandBlock / BlockManager — 命令块模型 (220 行)
├── image.rs            # 图片协议支持 — Kitty Graphics (1356 行)
├── bracketed_paste.rs  # BracketedPasteHandler — 括号粘贴模式 (130 行)
├── search.rs           # SearchState + search_grid — 回滚缓冲区搜索 (214 行)
├── bidi.rs             # BidiLine — 双向文本处理（UAX #9）(327 行)
├── replay.rs           # InstantReplay — 终端会话回放 (168 行)
├── parser/             → parser/AGENTS.md — VTE 解析执行引擎
├── screen/             → screen/AGENTS.md — 字符网格、样式、选择
├── input/              → input/AGENTS.md — 按键编码、鼠标、粘贴
└── tmux/               → tmux/AGENTS.md — Tmux 控制模式集成
```

## 模块详情

### lib.rs — crate 入口

声明所有子模块并通过 `pub use *` 重导出关键类型：Hyperlink、Mark/MarkKind、TermMode、ScreenSnapshot、Terminal、tmux 类型等。

### term.rs — Terminal 主结构体

`Terminal` 组合了 `VtePerform` + `VteParser` + `SearchState`，是外部 crate 的主要交互接口。

| 方法 | 说明 |
|------|------|
| `new(cols, rows, scrollback)` | 创建终端实例 |
| `write(data)` | 输入字节流，经解析器处理 |
| `resize(cols, rows)` | 调整终端尺寸（触发 Grid reflow） |
| `snapshot() -> ScreenSnapshot` | 获取当前可见屏幕快照 |
| `is_dirty()` / `clear_dirty()` | 脏标记管理 |
| `title()` / `current_dir()` | 终端标题和当前目录 |
| `mode() -> &TermMode` | 获取当前终端模式标志 |
| `take_bell() -> bool` | 检查并清除 bell 触发标志 |
| `scroll_up/down(lines)` / `scroll_to_bottom()` | 视口滚动 |
| `search(query)` / `search_next()` / `search_prev()` | 正则搜索与导航 |
| `get_text(start, end)` / `get_text_block(...)` | 提取文本（线性/矩形选择） |
| `get_visible_text()` | 获取全部可见文本 |
| `focus_in/out_sequence()` | 焦点事件转义序列 |
| `kitty_keyboard_flags()` | Kitty 键盘协议标志位 |
| `block_manager()` / `block_manager_mut()` | 访问 CommandBlock 管理器 |
| `next_block_prompt()` / `prev_block_prompt()` | 命令块间导航 |

### mode.rs — 终端模式标志

`TermMode`（u32 位标志），支持 `contains` / `insert` / `remove` / `set` 操作，实现 `BitOr` / `BitAnd` / `Not`。

| 常量 | 值 | 说明 |
|------|-----|------|
| `SHOW_CURSOR` | 0x0001 | 显示光标 |
| `APP_CURSOR` | 0x0002 | 应用光标键模式 |
| `APP_KEYPAD` | 0x0004 | 应用键盘模式 |
| `AUTO_WRAP` | 0x0008 | 自动换行 |
| `LINE_FEED_NEW_LINE` | 0x0010 | LF 同时执行 CR |
| `ORIGIN` | 0x0020 | 原点模式 |
| `COLUMN_132` | 0x0040 | 132 列模式 |
| `INVERSE` | 0x0080 | 反色显示 |
| `VISIBLE_BELL` | 0x0100 | 可视响铃 |
| `MOUSE_REPORT_CLICK` | 0x0200 | 鼠标点击报告 |
| `MOUSE_DRAG` | 0x0400 | 鼠标拖拽报告 |
| `MOUSE_MOTION` | 0x0800 | 鼠标移动报告 |
| `MOUSE_SGR` | 0x1000 | SGR 鼠标编码 |
| `BRACKETED_PASTE` | 0x2000 | 括号粘贴 |
| `FOCUS_IN_OUT` | 0x4000 | 焦点事件报告 |
| `INSERT` | 0x8000 | 插入模式（IRM） |
| `CURSOR_BLINK` | 0x10000 | 光标闪烁 |
| `LEFT_RIGHT_MARGIN` | 0x20000 | 左右边距模式 |
| `MOUSE_X10` | 0x40000 | X10 鼠标协议 |
| `SYNCHRONIZED_OUTPUT` | 0x80000 | 同步输出模式 |
| `MOUSE_SGR_PIXELS` | 0x100000 | SGR 像素鼠标 |
| `MODIFY_OTHER_KEYS` | 0x200000 | modifyOtherKeys |
| `KITTY_KEYBOARD_*` | 0x400000+ | Kitty 键盘协议标志（5 个 bit） |

### charset.rs — 字符集映射

支持 G0-G3 四个字符集槽位，锁定移位（LS）和单移位（SS）操作。

| 类型 | 说明 |
|------|------|
| `CharsetIndex` | G0 / G1 / G2 / G3 |
| `Charset` | ASCII、DecSpecialGraphics、UK、Portuguese 等 14 种字符集 |
| `dec_graphics_char(byte)` | ASCII 到 DEC Special Graphics 字符映射 |
| `map_char(charset, c)` | 按字符集转换字符 |
| `charset_from_designation(byte)` | 从 ESC 序列指定字节解析字符集 |

### hyperlink.rs — OSC 8 超链接

| 字段/方法 | 说明 |
|------|------|
| `Hyperlink::id` | 全局唯一 ID（AtomicU64 自增） |
| `Hyperlink::uri` | URI（如 `https://example.com`） |
| `Hyperlink::params` | 可选参数（如 `id=xyz`） |
| `parse_params()` | 解析 params 字符串为 HashMap |

### mark.rs — 语义标记

OSC 133 Shell 集成标记，用于命令生命周期追踪。

| 类型 | 说明 |
|------|------|
| `MarkKind` | PromptStart(A)、CommandStart(B)、OutputStart(C)、OutputEnd(D)、User |
| `Mark` | kind + label + exit_code + duration_micros |
| `from_osc133(marker)` | 从 OSC 133 字符串解析标记（含 exit/duration 扩展参数） |

### block.rs — 命令块模型

基于 OSC 133 标记的命令块生命周期管理。

| 类型 | 说明 |
|------|------|
| `CommandBlock` | 命令块：id、prompt_row、command_row、output_start/end_row、command_text、exit_code、duration_ms、collapsed |
| `BlockManager` | 管理命令块集合：on_prompt_start/command_start/output_start/output_end 事件驱动 |
| `block_at_row(row)` | 查找指定行所属的命令块 |
| `next_prompt_row()` / `prev_prompt_row()` | 命令块间前后导航 |

**生命周期：** PromptStart(A) -> CommandStart(B) -> OutputStart(C) -> OutputEnd(D)，下一个 PromptStart 自动完成当前块。

### image.rs — 图片协议支持（1356 行）

Kitty Graphics Protocol 的完整实现，支持 APC 转义序列格式的内联图片。

| 类型 | 说明 |
|------|------|
| `TerminalImage` | 终端网格中的放置图片：id、尺寸、RGBA 数据、位置、z_index |
| `ImageFormat` | Rgba / Rgb / Png / Direct |
| `KittyGraphicsCommand` | 解析后的 Kitty 图形命令 |
| `ImageManager` | 管理终端中的图片集合，支持放置、查询、删除 |

默认最大存储 320 MiB（Kitty 默认限制）。

### bracketed_paste.rs — 括号粘贴模式

| 方法/类型 | 说明 |
|------|------|
| `BracketedPasteHandler::wrap(content, enabled)` | 用 `\x1b[200~` / `\x1b[201~` 包裹粘贴内容 |
| `process_input(data)` | 检测输入中的粘贴标记，返回 `Vec<InputSegment>` |
| `InputSegment` | Raw（原始键盘输入）或 Paste（粘贴内容） |

### search.rs — 回滚缓冲区搜索

基于 Ghostty 架构的正则搜索系统。

| 类型 | 说明 |
|------|------|
| `GridPos` | 网格位置（row, col） |
| `SearchMatch` | 搜索匹配：start/end GridPos |
| `SearchDirection` | Forward / Backward |
| `SearchState` | 搜索状态：query、matches、selected、direction |
| `search_grid(regex, visible, history, rows)` | 跨可见区域和历史行搜索 |
| `search_visible(regex, visible)` | 仅搜索可见行 |

内部使用 `extract_row_text()` 提取行文本（跳过 SpacerTail/SpacerHead），`build_char_to_col_map()` 构建字符到列的映射。

### bidi.rs — 双向文本处理（327 行）

基于 `unicode_bidi` crate（UAX #9）的行级双向文本重排。

| 类型 | 说明 |
|------|------|
| `BidiLine` | 行级 visual-to-logical 映射 |
| `visual_to_logical` | Vec<usize>，视觉列索引 -> 逻辑列索引 |
| `has_rtl` | 是否包含 RTL 文本 |
| `visual_col(logical)` | 逻辑列 -> 视觉列 |
| `logical_col(visual)` | 视觉列 -> 逻辑列 |

处理策略：提取非 spacer 单元格字符 -> UAX #9 分析 -> 按解析层级重排 -> spacer 单元格跟随其主字符。纯 LTR 行返回恒等映射。

### replay.rs — 终端会话回放

时间旅行调试用的屏幕快照环形缓冲区。

| 类型 | 说明 |
|------|------|
| `ReplayFrame` | 单个快照：rows（Vec<String>）、cursor_row/col、timestamp |
| `InstantReplay` | 环形缓冲区管理器，默认 600 帧（~60 秒，100ms 间隔） |
| `try_capture()` | 遵守捕获间隔的快照 |
| `force_capture()` | 强制捕获（忽略间隔） |
| `step_back(n)` | 从最新帧向后 N 步 |

## 子模块链接

- [parser/AGENTS.md](parser/AGENTS.md) — VTE 解析执行引擎（VtePerform 1263 行）
- [screen/AGENTS.md](screen/AGENTS.md) — 字符网格、样式、选择区域
- [input/AGENTS.md](input/AGENTS.md) — 按键编码、鼠标事件、粘贴处理
- [tmux/AGENTS.md](tmux/AGENTS.md) — Tmux 控制模式集成

## 常见任务

### 添加新的终端模式
1. 在 `mode.rs` 的 `TermMode` 中添加新的 `pub const` 位标志
2. 在 `parser/perform.rs` 的 SM/RM 处理中添加对应的设置/清除逻辑
3. 在 `term.rs` 中暴露查询方法（如需要）

### 添加新的 OSC 序列
1. 在 `parser/perform.rs` 的 `osc_dispatch()` 中添加新的 command 分支
2. 在对应模块中添加处理逻辑

### 添加新的图片协议
1. 在 `image.rs` 中扩展解析和渲染逻辑
2. 在 `parser/perform.rs` 的 DCS/APC hook/put/unhook 中集成

### 调试字符显示问题
1. 检查 `parser/perform.rs` 中的 `print()` 方法和 `pending_wrap` 机制
2. 检查 `bidi.rs` 的 visual-to-logical 映射
3. 检查 `charset.rs` 的字符集转换
4. 参考 [parser/AGENTS.md](parser/AGENTS.md) 中的调试指南

### 调试搜索功能
1. 检查 `search.rs` 中的 `search_grid()` 逻辑
2. 验证 `extract_row_text()` 正确跳过 spacer 单元格
3. 检查 `build_char_to_col_map()` 的列映射
