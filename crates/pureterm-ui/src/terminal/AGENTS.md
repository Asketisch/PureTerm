# pureterm-ui/src/terminal — TerminalPane 终端面板

TerminalPane 是 pureterm-ui 的核心组件，组合 `Terminal`（终端模拟）+ `Session`（连接会话）。负责输入编码、鼠标事件处理、输出处理、选择和搜索。

## 文件结构

```
src/
├── mod.rs    # 模块重导出 (3 行)
└── pane.rs   # TerminalPane + Selection + SearchState (829 行)
```

## 模块依赖

```
terminal/ → pureterm-terminal/（Terminal、VtePerform）
terminal/ → pureterm-session/（Session trait）
```

## 核心类型

### TerminalPane（pane.rs）

**字段：**

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | `u64` | 全局唯一窗格 ID |
| `terminal` | `Terminal` | 终端模拟引擎 |
| `session` | `Option<Box<dyn Session>>` | 连接会话 |
| `selection` | `Option<Selection>` | 当前文本选择 |
| `search` | `SearchState` | 搜索状态 |
| `zoom` | `f32` | 缩放比例（0.5-3.0） |
| `title` | `String` | 窗格标题 |
| `has_focus` | `bool` | 是否拥有焦点 |
| `has_activity` | `bool` | 是否有未读活动 |
| `visual_bell_active` | `bool` | 可视铃声是否激活 |
| `dirty` | `bool` | 是否需要重绘 |
| `output_buffer` | `Arc<Mutex<Vec<u8>>>` | 输出缓冲（后台线程写入，主线程 drain） |
| `mouse_state` | `MouseState` | 内部鼠标状态（拖拽选择、双击检测） |
| `dropped_files` | `Vec<String>` | 拖拽到终端的文件 |
| `copy_on_select` | `bool` | 选择时自动复制 |
| `on_copy` | `Option<Box<dyn FnMut(&str) + Send>>` | 复制回调 |

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new(cols, rows, scrollback)` | 创建面板 |
| `set_session(session)` | 设置会话并启动输出处理 |
| `set_session_with_receiver(session, receiver)` | 使用预创建的接收器设置会话（避免 broadcast 竞争） |
| `drain_output()` | 从 output_buffer 读取数据并写入 terminal |
| `write_input(data)` | 写入 session |
| `handle_key(key, ctrl, alt, shift)` | 编码按键为转义序列并写入 session |
| `encode_key(key, ctrl, alt, shift)` | 按键到转义序列的映射（见下表） |
| `resize(cols, rows)` | 调整 terminal 和 session |
| `copy_selection()` / `paste(text)` | 剪贴板操作 |
| `zoom_in()` / `zoom_out()` / `zoom_reset()` | 缩放（范围 0.5-3.0） |
| `update_search()` | 搜索可见文本 |
| `handle_mouse_event(event)` | 处理鼠标事件，返回待发送数据和 PaneAction |
| `url_at(row, col)` | 获取指定位置的超链接 |
| `pixel_to_cell(px, py, cell_w, cell_h)` | 像素坐标转单元格坐标 |

### Selection（pane.rs）

文本选择区域：

```rust
pub struct Selection {
    pub start_row: usize, pub start_col: usize,
    pub end_row: usize, pub end_col: usize,
    pub is_block: bool,
}
```

`contains(row, col)` 判断单元格是否在选中区域内。

### SearchState（pane.rs）

```rust
pub struct SearchState {
    pub query: String,
    pub matches: Vec<(usize, usize, usize)>, // (row, start_col, end_col)
    pub current_match: Option<usize>,
    pub case_sensitive: bool,
    pub regex: bool,
}
```

### MouseState（pane.rs，内部）

内部鼠标状态，跟踪拖拽选择和双击/三击检测：

```rust
struct MouseState {
    left_down: bool,
    drag_start: Option<(usize, usize)>,
    last_click_time: Option<Instant>,
    last_click_pos: Option<(usize, usize)>,
    click_count: u8,  // 1=single, 2=double, 3=triple
}
```

## 按键编码（encode_key）

### 控制字符（Ctrl + 字母）

| 键 | 输出 |
|---|------|
| Ctrl+A-Z | 0x01-0x1A |
| Ctrl+[ | 0x1B (ESC) |
| Ctrl+\\ | 0x1C (FS) |
| Ctrl+] | 0x1D (GS) |
| Ctrl+/ | 0x1F (US) |

### 修饰键 + 方向键

| 修饰键组合 | 方向键输出 |
|------------|------------|
| Ctrl+Arrow | `\x1b[1;5A/B/C/D` |
| Alt+Arrow | `\x1b[1;3A/B/C/D` |
| Ctrl+Shift+Arrow | `\x1b[1;6A/B/C/D` |
| Alt+Home/End | `\x1b[1;3H/F` |

### App Cursor 模式

| 键 | App Cursor ON (SS3) | App Cursor OFF (CSI) |
|---|-------------------|---------------------|
| Up | `\x1bOA` | `\x1b[A` |
| Down | `\x1bOB` | `\x1b[B` |
| Right | `\x1bOC` | `\x1b[C` |
| Left | `\x1bOD` | `\x1b[D` |
| Home | `\x1bOH` | `\x1b[H` |
| End | `\x1bOF` | `\x1b[F` |

### 功能键

| 键 | 输出 |
|---|------|
| F1-F4 | SS3: `\x1bOP/Q/R/S` |
| F5-F12 | CSI: `\x1b[15~` - `\x1b[24~` |
| Shift+Tab | `\x1b[Z` |

### Alt + 可打印字符

输出 `ESC + char`（0x1B 前缀）。

## 输出处理流程

```
Session → broadcast channel → 后台线程（start_output_processing）
    → output_buffer → drain_output() → Terminal::write()
```

`drain_output()` 在主线程调用，从 `output_buffer` 读取所有累积数据并写入 terminal。

## 输入时自动滚回底部

`handle_key()` 在编码前检查 `scroll_offset() > 0`，如有则 `scroll_to_bottom()` 并标记 dirty。

## 粘贴处理

`paste()` 先 `trim_end_matches('\n')` 去除尾部换行，再检查 `BRACKETED_PASTE` 模式决定是否包裹 `\x1b[200~`/`\x1b[201~`。

## 右键行为

有选区时先复制（返回 `PaneAction::CopyText`），否则显示上下文菜单。

## Alt + 滚轮

Alt 键按下时滚轮发送方向键序列（适用于 `less` 等程序）。

## 父级导航

← [pureterm-ui/AGENTS.md](../../AGENTS.md)
