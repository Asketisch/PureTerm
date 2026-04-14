# pureterm-terminal — VTE 解析器、屏幕网格、终端模拟

终端模拟引擎的核心实现：VTE 转义序列解析、字符网格管理（含 reflow）、光标操作、超链接、图片支持。

## 关键依赖

`vte`（VTE 解析）、`unicode-width` + `unicode-segmentation`（Unicode）、`serde`（序列化）、`thiserror`（错误）、`parking_lot`（并发）。

无内部依赖（standalone crate）。

## 文件结构

```
src/
├── lib.rs              # 模块导出 + pub use
├── term.rs            # Terminal 结构体（顶层接口）
├── mode.rs            # TermMode 位标志
├── hyperlink.rs       # 超链接支持（ OSC 8 ）
├── mark.rs            # 终端标记（书签）
├── image.rs           # 内联图片支持（ITerm2 协议）
├── bracketed_paste.rs # 括号粘贴模式
├── replay.rs          # 终端回放功能
├── parser/
│   ├── mod.rs          # VteParser 包装
│   └── perform.rs      # VtePerform（vte::Perform 实现）+ ScreenSnapshot
└── screen/
    ├── mod.rs
    ├── cell.rs         # Cell、CellAttrs、Color、NamedColor
    ├── color.rs        # 重导出 Color
    ├── cursor.rs       # Cursor、CursorStyle
    ├── grid.rs         # Grid（回滚缓冲区 + 可见视口 + wrapped flag + reflow）
    ├── selection.rs    # 选择区域 + 鼠标事件编码
    └── history.rs      # 回滚历史管理
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
| `resize(cols, rows)` | 调整终端尺寸（触发 Grid reflow） |
| `snapshot() -> ScreenSnapshot` | 获取当前可见屏幕快照 |
| `is_dirty()` / `clear_dirty()` | 脏标记管理 |
| `title()` / `current_dir()` | 终端标题和当前目录 |
| `mode() -> &TermMode` | 获取当前终端模式标志 |
| `take_bell() -> bool` | 检查并清除 bell 触发标志 |
| `scroll_up/down(lines)` / `scroll_to_bottom()` | 视口滚动 |
| `get_text(start_row, start_col, end_row, end_col)` | 提取文本（用于复制） |
| `get_visible_text()` | 获取全部可见文本 |

### mode.rs — 终端模式标志

`TermMode`（u32 位标志）：`SHOW_CURSOR`(0x01)、`APP_CURSOR`(0x02)、`APP_KEYPAD`(0x04)、`AUTO_WRAP`(0x08)、`LINE_FEED_NEW_LINE`(0x10)、`ORIGIN`(0x20)、`COLUMN_132`(0x40)、`INVERSE`(0x80)、`VISIBLE_BELL`(0x100)、`MOUSE_REPORT_CLICK`(0x200)、`MOUSE_DRAG`(0x400)、`MOUSE_MOTION`(0x800)、`MOUSE_SGR`(0x1000)、`BRACKETED_PASTE`(0x2000)、`FOCUS_IN_OUT`(0x4000)、`INSERT`(0x8000)、`CURSOR_BLINK`(0x10000)。

### screen/ — 字符网格与选择区域

详细文档：[screen/AGENTS.md](src/screen/AGENTS.md)

包含：Grid（字符网格 + reflow + 回滚）、Cell/CellAttrs/Color（单元格属性）、Cursor/CursorStyle（光标）、selection.rs（鼠标事件编码 + 单词选择）。

### parser/ — VTE 执行引擎

详细文档：[parser/AGENTS.md](src/parser/AGENTS.md)

`VtePerform` 实现 `vte::Perform` trait（1263 行），包含：CSI/OSC/ESC 序列分发、`pending_wrap` 机制、IRM 插入模式、双宽字符、备用屏幕缓冲区（Alt Screen）、DCS Hook/Put/Unhook、SGR 文本属性。ScreenSnapshot 作为渲染用的屏幕快照。

## 常见任务

详细调试指南请参考子模块 AGENTS.md：

- [parser/AGENTS.md](src/parser/AGENTS.md) — VTE 解析行为、CSI 指令添加、字符显示调试、光标定位调试
- [screen/AGENTS.md](src/screen/AGENTS.md) — 网格修改、reflow 逻辑、鼠标事件编码

### 处理 Bell 事件
UI 层通过 `terminal.take_bell()` 轮询检查 bell 触发。返回 true 后标志自动清除。
