# 16 - Tmux 控制模式

## 功能概述

Tmux 控制模式的客户端实现：解析 tmux 输出、管理会话/窗口/面板、布局解析、远程终端视图。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `terminal/tmux/control.zig` | ~500 | 控制模式解析器 |
| `terminal/tmux/layout.zig` | ~300 | 布局字符串解析器 |
| `terminal/tmux/output.zig` | ~200 | 格式化输出解析 |
| `terminal/tmux/viewer.zig` | ~800 | Tmux 会话查看器 |

## 核心功能

### 1. 控制模式解析器 (`control.zig`)

```
解析 tmux DCS 1000p 输出:
  %session-changed <id> <name>
  %layout-change <win-id> <layout>
  %output <pane-id> <data>
  %exit [<reason>]
  %window-add <win-id>
  %window-close <win-id>
  %pane-mode <pane-id> <mode>
  %begin <id>          - 命令输出开始
  %end <id>            - 命令输出结束
  %error <id> <msg>    - 命令错误
```

### 2. 布局解析器 (`layout.zig`)

```
解析格式: WxH,X,Y{...}[...]

Layout (union):
  ├── pane: PaneInfo     - 叶子节点
  │     ├── id: []const u8
  │     ├── x, y, w, h: u16
  │     └── active: bool
  ├── horizontal: Split  - 水平分割
  │     ├── children: []Layout
  │     └── sizes: []u16
  └── vertical: Split    - 垂直分割

支持校验和验证
```

### 3. 查看器 (`viewer.zig`)

```
Viewer:
  ├── session: SessionInfo
  ├── windows: []WindowInfo
  ├── panes: []PaneInfo
  ├── terminal: Terminal   - 远程终端视图
  ├── state: enum { detached, attaching, attached }
  └── 方法:
        ├── attach()        - 连接到会话
        ├── detach()        - 断开
        ├── refreshLayout() - 刷新布局
        ├── switchPane()    - 切换面板
        └── processOutput() - 处理 tmux 输出
```

## 纯 Rust 实现要点

**必须实现**。Tmux 控制模式是重要的终端功能，必须全部实现。需要注意：
- 解析器是简单的字符串分割
- 布局树是递归结构
- 查看器需要维护一个远程 Terminal 实例
