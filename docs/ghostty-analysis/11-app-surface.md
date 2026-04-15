# 11 - 应用运行时与 Surface

## 功能概述

应用生命周期管理、终端 Surface（单个终端实例）、跨平台 UI 运行时抽象、窗口/标签/分割管理。

## Ghostty 源码文件

### 核心 (4)
| 文件 | 行数 | 功能 |
|------|------|------|
| `App.zig` | ~624 | **应用主结构**：管理所有 Surface、共享字体、消息路由 |
| `Surface.zig` | ~6660 | **核心 Surface**：终端实例，最大文件 |
| `surface_mouse.zig` | ~337 | 鼠标辅助：光标形状状态机 |
| `apprt.zig` | ~59 | 运行时包根 |

### 运行时抽象 (9)
| 文件 | 功能 |
|------|------|
| `apprt/runtime.zig` | 运行时枚举选择 |
| `apprt/action.zig` | 动作联合类型 |
| `apprt/surface.zig` | Surface 消息类型 |
| `apprt/structs.zig` | 共享数据结构 |
| `apprt/ipc.zig` | 进程间通信 |
| `apprt/gtk.zig` | GTK 运行时入口 |
| `apprt/embedded.zig` | 嵌入式运行时 (macOS) |
| `apprt/none.zig` | 无头运行时 |
| `apprt/browser.zig` | 浏览器运行时 |

### GTK 运行时 (22 + 19 类)
| 文件 | 功能 |
|------|------|
| `apprt/gtk/App.zig` | GTK App 包装 |
| `apprt/gtk/Surface.zig` | GTK Surface 包装 |
| `apprt/gtk/key.zig` | 键盘映射 |
| `apprt/gtk/class.zig` | GObject 类系统 |
| `apprt/gtk/class/application.zig` | 主 Application 类 |
| `apprt/gtk/class/window.zig` | Window 类 |
| `apprt/gtk/class/surface.zig` | Surface widget 类 |
| `apprt/gtk/class/tab.zig` | Tab 类 |
| `apprt/gtk/class/split_tree.zig` | 分割树类 |
| `apprt/gtk/class/dialog.zig` | 对话框抽象 |
| `apprt/gtk/class/search_overlay.zig` | 搜索覆盖层 |
| `apprt/gtk/class/resize_overlay.zig` | 大小调整覆盖层 |
| `apprt/gtk/class/close_confirmation_dialog.zig` | 关闭确认 |
| `apprt/gtk/class/clipboard_confirmation_dialog.zig` | 剪贴板确认 |
| `apprt/gtk/class/title_dialog.zig` | 标题设置对话框 |
| `apprt/gtk/class/config_errors_dialog.zig` | 配置错误对话框 |
| `apprt/gtk/class/command_palette.zig` | 命令面板 |
| `apprt/gtk/class/key_state_overlay.zig` | 按键状态覆盖层 |
| `apprt/gtk/class/inspector_widget.zig` | 检查器 widget |
| `apprt/gtk/class/inspector_window.zig` | 检查器窗口 |
| `apprt/gtk/class/debug_warning.zig` | 调试警告横幅 |
| `apprt/gtk/class/config.zig` | Config GObject 包装 |

### 窗口协议 (5)
| 文件 | 功能 |
|------|------|
| `apprt/gtk/winproto.zig` | 协议选择（X11/Wayland） |
| `apprt/gtk/winproto/wayland.zig` | Wayland 实现 |
| `apprt/gtk/winproto/x11.zig` | X11 实现 |
| `apprt/gtk/winproto/noop.zig` | 空实现 |
| `apprt/gtk/winproto/BlurRegion.zig` | 模糊区域计算 |

## 核心数据结构

### 1. App (`App.zig`)

```
App:
  ├── alloc: Allocator
  ├── surfaces: SurfaceList        - 所有活跃 Surface
  ├── focused: ?*Surface           - 当前聚焦的 Surface
  ├── mailbox: Mailbox             - 应用级消息
  ├── font_grid_set: SharedGridSet - 共享字体网格
  ├── config_conditional_state     - 条件配置状态
  └── 方法:
        ├── create()               - 创建应用
        ├── tick()                 - 事件循环 tick
        ├── updateConfig()         - 配置变更
        ├── addSurface()           - 添加 Surface
        ├── deleteSurface()        - 删除 Surface
        ├── keyEvent()             - 处理按键
        ├── performAction()        - 执行动作
        └── colorSchemeEvent()     - 主题变更
```

### 2. Surface (`Surface.zig`) ⭐ 核心文件

```
Surface (6660 行):
  ├── 终端状态:
  │     ├── io: Termio             - 终端 I/O
  │     ├── renderer: Renderer     - 渲染器
  │     ├── renderer_state: State  - 渲染状态
  │     ├── renderer_thread        - 渲染线程
  │     └── size: Size             - 当前大小
  ├── 字体:
  │     ├── font_grid_key          - 字体网格键
  │     ├── font_size: f32         - 当前字体大小
  │     └── font_metrics: Metrics  - 字体度量
  ├── 输入:
  │     ├── mouse: Mouse           - 鼠标状态
  │     ├── keyboard: Keyboard     - 键盘状态
  │     ├── pressed_key            - 按下的键
  │     └── last_binding_trigger   - 最后绑定触发
  ├── UI 状态:
  │     ├── focused: bool
  │     ├── readonly: bool
  │     ├── child_exited: bool
  │     ├── selection_scroll_active: bool
  │     └── config: DerivedConfig
  └── 搜索/检查器:
        ├── search: Search
        ├── inspector: ?Inspector
        └── command_timer
```

**主要回调方法** (30+):
- `keyCallback()` - 键盘事件
- `textCallback()` - 文本输入
- `mouseButtonCallback()` - 鼠标按钮
- `cursorPosCallback()` - 鼠标移动
- `scrollCallback()` - 滚轮
- `focusCallback()` - 焦点变化
- `sizeCallback()` - 大小变化
- `contentScaleCallback()` - DPI 变化
- `preeditCallback()` - IME 预编辑
- `colorSchemeCallback()` - 主题变化
- `occlusionCallback()` - 可见性

### 3. Surface 鼠标 (`surface_mouse.zig`)

```
鼠标光标形状状态机:
  默认: text (I-beam)
  + Ctrl: default (arrow)
  + Super: default
  + 链接上: pointer (手指)
  + 矩形选择: crosshair
  + 鼠标模式: default (不改变)
  + 隐藏: 不显示
```

### 4. 动作系统 (`apprt/action.zig`)

```
Action (联合类型):
  ├── quit
  ├── new_window
  ├── new_tab
  ├── close_tab
  ├── new_split: SplitDirection
  ├── toggle_fullscreen
  ├── toggle_maximize
  ├── toggle_quick_terminal
  ├── move_tab: isize
  ├── goto_tab: isize
  ├── resize_split: SplitResizeParameter
  ├── cell_size: CellSize
  └── ... 更多

Target: app | surface
```

## 纯 Rust 实现要点

1. **App 用 egui/winit**：管理窗口生命周期
2. **Surface 是核心结构**：包含终端状态、渲染器、IO
3. **GTK 运行时不需要**：Rust 直接用 winit + egui/wgpu
4. **动作系统用 enum**：匹配分发
5. **消息系统用 channel**：`std::sync::mpsc` 或 `crossbeam`

### Rust 架构

```rust
pub struct App {
    surfaces: Vec<Surface>,
    font_grid_set: FontGridSet,
    config: Arc<Config>,
    focused: Option<SurfaceId>,
    mailbox: Receiver<AppMessage>,
}

pub struct Surface {
    id: SurfaceId,
    terminal: Terminal,
    termio: Termio,
    renderer: Renderer,
    font_grid: Arc<SharedGrid>,
    mouse: SurfaceMouse,
    keyboard: KeyboardState,
    config: DerivedConfig,
    // ...
}
```
