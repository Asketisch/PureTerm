# pureterm-terminal/src/tmux — Tmux 控制模式集成

Tmux 控制模式支持，用于与 tmux 通信并获取布局和输出信息。

## 文件结构

```
src/
├── mod.rs         # 模块重导出 (27 行)
├── control.rs     # DCS 1000p 控制模式解析器 (592 行)
├── layout.rs      # 布局字符串解析器 (389 行)
└── viewer.rs      # Tmux 会话查看器 (383 行)
```

## 模块依赖

```
tmux/ → parser/（VteParser，用于输出解析）
tmux/ → screen/（Grid，用于格式化输出）
tmux/ → lib.rs（Hyperlink）
```

## 核心类型

### ControlParser（control.rs）

DCS 1000p 输出解析器：

```rust
pub struct ControlParser {
    state: ControlParserState,
    buffer: Vec<u8>,
    current_event: Option<ControlEvent>,
}
```

**状态机状态：**
- `Idle` — 等待 `%` 开始
- `ReadingPercent` — 读取事件名称
- `ReadingArgs` — 读取参数
- `InEscape` — ESC 转义序列中

**关键方法：**

| 方法 | 说明 |
|------|------|
| `parse(&mut self, data: &[u8])` | 解析数据，返回事件列表 |
| `reset(&mut self)` | 重置解析器状态 |

### ControlEvent（control.rs）

控制事件类型：

```rust
pub enum ControlEvent {
    SessionChanged { id: u64, name: String },
    LayoutChange { window_id: u64, layout: String },
    Output { pane_id: u64, data: String },
    Exit { reason: Option<String> },
    WindowAdd { window_id: u64 },
    WindowClose { window_id: u64 },
    PaneMode { pane_id: u64, mode: String },
    Begin { id: u64 },
    End { id: u64 },
    Error { id: u64, message: String },
    Unknown { name: String, args: Vec<String> },
}
```

### TmuxCommand（control.rs）

发送到 tmux 的命令：

```rust
pub enum TmuxCommand {
    SelectWindow { index: usize },
    SelectPane { id: u64 },
    SendKeys { pane_id: Option<u64>, keys: String },
    ResizePane { pane_id: Option<u64>, width: usize, height: usize },
    DetachClient { name: Option<String> },
    ListSessions,
    ListWindows { session: Option<String> },
    ListPanes { window: Option<String> },
    NewSession { name: Option<String>, start_directory: Option<String> },
    NewWindow { name: Option<String> },
    KillPane { pane_id: u64 },
    KillWindow { window_id: u64 },
}
```

**命令编码（`encode_command`）：**
```rust
encode_command(TmuxCommand::SendKeys { pane_id: Some(1), keys: "ls" })
// → "\x1bP1000p\x1b\\send-keys -t 1 ls\x1b\\"
```

### LayoutParser（layout.rs）

布局字符串解析器：

```rust
pub struct LayoutParser {
    input: Vec<char>,
    pos: usize,
}
```

**布局格式：**
```
{WxH,X,Y[%id],...}    # 水平分割（大括号）
[WxH,X,Y[%id]]        # 垂直分割（中括号）
WxH,X,Y[%id]          # 叶子窗格（无括号）
```

**解析结果：**
```rust
pub enum Layout {
    Pane(PaneInfo),      // 叶子窗格
    Split(SplitInfo),    // 分割容器
}

pub struct PaneInfo {
    pub id: String,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub active: bool,
}

pub struct SplitInfo {
    pub split_type: SplitType,  // Horizontal / Vertical
    pub children: Vec<Layout>,
    pub sizes: Vec<u16>,        // 每个子元素的尺寸
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `parse(input: &str) -> Result<Layout>` | 解析布局字符串 |
| `parse_pane(size: &str)` | 解析窗格信息 |
| `parse_split(size: &str)` | 解析分割容器 |
| `flatten(layout) -> Vec<PaneInfo>` | 展平为窗格列表 |
| `bounds(layout) -> (u16, u16, u16, u16)` | 计算边界框 |

### Viewer（viewer.rs）

Tmux 会话查看器，管理连接和状态：

```rust
pub struct Viewer {
    state: ViewerState,
    sessions: HashMap<u64, SessionInfo>,
    windows: HashMap<u64, WindowInfo>,
    panes: HashMap<u64, PaneInfo>,
    active_session: Option<u64>,
    active_window: Option<u64>,
}
```

**连接状态：**

```rust
pub enum ViewerState {
    Disconnected,  // 未连接
    Connecting,    // 连接中
    Attached,      // 已连接
    Detaching,     // 断开中
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `connect(session_name) -> Result<()>` | 连接到 tmux 会话 |
| `disconnect()` | 断开连接 |
| `process_raw_output(line: &str)` | 处理 tmux 输出行 |
| `handle_event(event: ControlEvent)` | 处理控制事件 |
| `get_active_panes() -> Vec<PaneInfo>` | 获取活动窗格列表 |
| `send_command(cmd: TmuxCommand)` | 发送命令到 tmux |

## Tmux 控制模式协议

### 启用控制模式

```bash
tmux -C attach -t <session>
# 或
tmux -C new -s <session>
```

### 输出格式

每行以 `%` 开头：
```
%session-changed <id> <name>
%layout-change <win-id> <layout>
%output <pane-id> <data>
%exit [<reason>]
```

### 命令格式

DCS 1000p 封装：
```
\x1bP1000p<command>\x1b\\
```

## 与 Terminal 集成

`Viewer` 可以与 `Terminal` 结合使用：

```rust
// 创建 tmux viewer
let viewer = Viewer::new();

// 连接到 tmux 会话
viewer.connect("default")?;

// 处理 tmux 输出
viewer.process_raw_output(line);

// 获取布局并渲染
for pane in viewer.get_active_panes() {
    let snapshot = terminal.snapshot();
    renderer.render_pane(pane, snapshot);
}
```

## 常见任务

### 添加新的控制事件

1. 在 `ControlEvent` 枚举中添加变体
2. 在 `ControlParser::parse_event()` 中添加解析逻辑
3. 在 `Viewer::handle_event()` 中添加处理逻辑

### 修改布局解析

编辑 `layout.rs` 的 `LayoutParser` 方法。注意 `parse_maybe_split` 中的 lookahead 逻辑用于区分窗格 ID (`[%id]`) 和分割标记 (`[...]`)。

### 调试 tmux 集成问题

1. 检查 `ControlParser` 状态机是否正确处理 ESC 序列
2. 验证布局字符串格式：`{80x24,0,0[%0],80x24,80,0[%1]}0,0`
3. 检查 `Viewer::process_raw_output()` 是否正确分割输出行

## 父级导航

← [pureterm-terminal/AGENTS.md](../../AGENTS.md)