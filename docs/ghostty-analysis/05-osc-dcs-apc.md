# 05 - OSC/DCS/APC 序列处理

## 功能概述

处理终端的 OSC (Operating System Command)、DCS (Device Control String)、APC (Application Program Command) 序列。这些序列控制窗口标题、剪贴板、超链接、颜色、通知、Shell 集成标记等高级功能。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `terminal/osc.zig` | ~1000 | OSC 解析器状态机 |
| `terminal/dcs.zig` | ~300 | DCS 处理器 |
| `terminal/apc.zig` | ~200 | APC 处理器（Kitty 图形） |
| `terminal/osc/parsers/change_window_title.zig` | ~50 | OSC 0/2: 窗口标题 |
| `terminal/osc/parsers/change_window_icon.zig` | ~30 | OSC 1: 窗口图标 |
| `terminal/osc/parsers/report_pwd.zig` | ~50 | OSC 7: 报告当前目录 |
| `terminal/osc/parsers/hyperlink.zig` | ~100 | OSC 8: 超链接 |
| `terminal/osc/parsers/clipboard_operation.zig` | ~100 | OSC 52: 剪贴板操作 |
| `terminal/osc/parsers/color.zig` | ~200 | OSC 4/5/10-19/104/105/110-119: 颜色 |
| `terminal/osc/parsers/osc9.zig` | ~80 | OSC 9: 桌面通知 |
| `terminal/osc/parsers/semantic_prompt.zig` | ~100 | OSC 133: Shell 集成标记 |
| `terminal/osc/parsers/mouse_shape.zig` | ~30 | OSC 22: 鼠标光标形状 |
| `terminal/osc/parsers/kitty_color.zig` | ~200 | OSC 21: Kitty 颜色协议 |
| `terminal/osc/parsers/kitty_clipboard_protocol.zig` | ~150 | OSC 5522: Kitty 剪贴板 |
| `terminal/osc/parsers/kitty_text_sizing.zig` | ~80 | OSC 66: Kitty 文本尺寸 |
| `terminal/osc/parsers/rxvt_extension.zig` | ~50 | OSC 777: rxvt 扩展通知 |
| `terminal/osc/parsers/iterm2.zig` | ~100 | OSC 1337: iTerm2 扩展 |
| `terminal/osc/parsers/context_signal.zig` | ~100 | OSC 3008: 上下文信号 |
| `terminal/osc/encoding.zig` | ~30 | Kitty 安全 UTF-8 验证 |

## OSC 序列完整清单

### OSC 解析器状态机

```
状态:
  ├── start             - 初始，等待命令号
  ├── command           - 收集命令号
  ├── data              - 收集数据
  └── end               - 序列结束，分发处理

支持的终止符: BEL (0x07) 或 ST (ESC \)
参数分隔符: ;
```

### 命令详情

#### OSC 0 / OSC 2 - 窗口标题
```
OSC 0 ; <title> ST     - 设置图标名和标题
OSC 2 ; <title> ST     - 设置窗口标题
```

#### OSC 7 - 报告当前目录
```
OSC 7 ; <url> ST       - 设置当前工作目录 (file://host/path)
```

#### OSC 8 - 超链接
```
OSC 8 ; params ; uri ST    - 开始超链接
OSC 8 ; ; ST               - 结束超链接
params: id=<value>
```

#### OSC 52 - 剪贴板
```
OSC 52 ; <clipboard> ; <base64_data> ST
clipboard: c(剪贴板), p(主选择), s(选择)
base64_data 为空时 = 读取请求
base64_data 为 ? 时 = 查询
```

#### OSC 9 - 桌面通知
```
OSC 9 ; 4 ; <progress> ST  - 进度条 (ConEmu)
OSC 9 ; <message> ST        - 通知消息
```

#### OSC 21 - Kitty 颜色协议
```
OSC 21 ; key=value ; key=value ST
key: fg, bg, cursor, selection_fg, selection_bg, etc.
value: hex color, ? (query), - (reset)
```

#### OSC 22 - 鼠标光标形状
```
OSC 22 ; <shape> ST     - 设置鼠标光标
shape: W3C cursor name (default, text, pointer, crosshair, etc.)
```

#### OSC 133 - 语义提示标记 (Shell 集成)
```
OSC 133 ; A ST          - 提示开始
OSC 133 ; B ST          - 提示结束 / 命令开始
OSC 133 ; C ST          - 命令执行开始
OSC 133 ; D [; exit] ST - 命令输出结束 [带退出码]
```

#### OSC 4/5/10-19 - 颜色设置/查询
```
OSC 4 ; <index> ; <color> ST       - 设置调色板颜色
OSC 10 ; <color> ST                - 设置前景色
OSC 11 ; <color> ST                - 设置背景色
OSC 12 ; <color> ST                - 设置光标颜色
OSC 104 ; <index> ST               - 重置调色板颜色
OSC 110-119 ST                     - 重置特殊颜色
```

#### OSC 777 - rxvt 通知
```
OSC 777 ; notify ; <title> ; <body> ST
```

#### OSC 1337 - iTerm2 扩展
```
OSC 1337 ; SetMark ST              - 设置书签
OSC 1337 ; ShellIntegrationVersion ; <ver> ST
OSC 1337 ; CurrentDir ; <url> ST   - 当前目录
OSC 1337 ; RemoteHost ; <user>@<host> ST
```

#### OSC 5522 - Kitty 剪贴板协议
```
OSC 5522 ; <action> ; <options> ; <data> ST
action: read, write, alias
```

#### OSC 3008 - 上下文信号
```
OSC 3008 ; start ; type=<value>;user=<value>;hostname=<value>;pid=<value> ST
OSC 3008 ; end ST
```

## DCS 处理

| 序列 | 功能 |
|------|------|
| `DCS 1000 p ... ST` | Tmux 控制模式 |
| `DCS + q <name> ST` | XTGETTCAP (请求终端能力) |
| `DCS $ q ... ST` | DECRQSS (请求设置状态) |

## APC 处理

| 序列 | 功能 |
|------|------|
| `APC G ... ST` | Kitty 图形协议 |

## 纯 Rust 实现要点

1. **OSC 解析器用状态机**：start → command → data → dispatch
2. **每个 OSC 命令一个处理函数**：`fn handle_osc_52(data: &str) -> Result<ClipboardOp>`
3. **OSC 8 超链接**：需要 hyperlink 管理器，在 Screen 中维护
4. **OSC 133 语义提示**：简单的标记系统，在 Cell 上添加 semantic 标志
5. **Base64 编解码**：用标准库或 `base64` crate
6. **DCS/APC 用类似的解析器结构**

### 实现架构

```rust
pub enum OscCommand {
    SetTitle(String),          // OSC 0/2
    SetPwd(Url),               // OSC 7
    Hyperlink(HyperlinkAction),// OSC 8
    Clipboard(ClipboardAction),// OSC 52
    Color(ColorAction),        // OSC 4/10/11/12
    Notification(String),      // OSC 9
    SemanticPrompt(SemanticAction), // OSC 133
    KittyColor(KittyColorAction),   // OSC 21
    MouseShape(String),        // OSC 22
    // ...
}

pub struct OscParser {
    state: OscState,
    command: u16,
    data: String,
}
```
