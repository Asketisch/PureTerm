# pureterm-ui — egui 组件层

基于 egui 的 UI 组件：应用状态管理、Surface 抽象（Ghostty 风格）、标签页、分割面板（SplitTree 不可变树 + SplitContainer 递归容器）、终端面板、命令面板、搜索、AI 聊天、专注模式、跳转提示、在线搜索、快速命令栏等。

src/ 级别模块详解：[src/AGENTS.md](src/AGENTS.md)

## 关键依赖

内部依赖：`pureterm-core`、`pureterm-terminal`、`pureterm-session`

外部依赖：`egui` + `epaint`（UI）、`tokio`（异步）、`serde`、`thiserror`、`parking_lot`、`uuid`（PaneId）。

## 文件结构

```
src/
├── lib.rs              # 模块导出
├── app.rs              # App 主应用状态 + AppMessage 消息循环 (178 行)
├── ai_chat.rs          # AI 聊天面板
├── autocomplete.rs      # 自动补全
├── clipboard.rs         # 剪贴板管理
├── command_palette.rs   # 命令面板（Ctrl+Shift+P）
├── context_menu.rs      # 右键菜单（17 个 MenuAction）
├── explorer.rs          # 文件浏览器（本地 + SFTP）
├── focus_mode.rs        # 专注模式（隐藏所有 UI 元素）(76 行)
├── history_menu.rs      # 命令历史菜单（VecDeque, MAX_HISTORY=10000）(279 行)
├── inspector.rs         # 终端检查器（调试面板）
├── jump_prompt.rs       # 跳转提示（Ctrl+; / OSC 133 prompt mark）(74 行)
├── lock_screen.rs       # 锁屏
├── online_search.rs     # 在线搜索面板 (135 行)
├── profile_selector.rs  # 连接配置选择器
├── quick_bar.rs         # 快速命令栏（Command/Search/Filter 三模式）(116 行)
├── search.rs            # 搜索面板（SlidingWindow + ViewportSearch）
├── settings/
│   ├── mod.rs          # 设置面板
│   └── AGENTS.md       # 设置面板详解
├── split/
│   ├── mod.rs           # 重导出（含 SplitSide、DividerInfo、divider_at_point 等）
│   ├── container.rs     # SplitContainer + SplitChild + Direction + SplitSide
│   ├── layout.rs        # compute_pane_rects、compute_divider_rects、divider_at_point、PaneRect
│   └── AGENTS.md        # 分割面板布局详解
├── split_tree.rs        # SplitTree 不可变持久化二叉树 (288 行)
├── surface.rs           # Surface 抽象（Ghostty 风格）(583 行)
├── tab_bar.rs           # 标签页栏 UI 组件
├── tab_recovery.rs      # 标签页恢复
├── tabs/
│   ├── mod.rs           # 重导出
│   ├── manager.rs       # TabManager + Tab + TabContent + ClosedTab
│   └── AGENTS.md        # 标签页管理详解
├── terminal/
│   ├── mod.rs           # 重导出
│   ├── pane.rs          # TerminalPane + Selection + ScrollState + PaneAction (1395 行)
│   └── AGENTS.md        # 终端面板详解
├── theme.rs             # 主题管理
├── tokens.rs            # UI token 定义
└── welcome.rs           # 欢迎页面
```

## 模块详情

### app.rs — 主应用状态

Ghostty `App.zig` 风格的应用状态管理，基于邮箱消息循环：

| 字段 | 类型 | 说明 |
|------|------|------|
| `config` | `AppConfig` | 应用配置 |
| `tab_manager` | `TabManager` | 标签页管理器 |
| `hotkey_service` | `HotkeyService` | 快捷键服务 |
| `surfaces` | `Vec<SurfaceId>` | Surface 注册表 |
| `focused_surface` | `Option<SurfaceId>` | 当前聚焦 Surface |
| `mailbox` | `Vec<AppMessage>` | 消息队列 |
| `show_settings` | `bool` | 设置面板可见性 |
| `search_visible` | `bool` | 搜索栏可见性 |
| `quitting` | `bool` | 退出标记 |

**AppMessage 枚举**（15 个变体）：Quit、NewWindow、NewTab、NewSplit、ToggleFullscreen、ConfigChanged、SurfaceDestroyed 等。

`handle_key(key, ctrl, alt, shift, super_key)` — 检查快捷键绑定，处理 tab_new、tab_close、search、prev_tab、next_tab。

### surface.rs — Surface 抽象（583 行）

Ghostty 风格的单终端 Surface 模型，封装 Terminal + 输入处理 + 鼠标/键盘状态 + 动作分发：

| 类型 | 说明 |
|------|------|
| `SurfaceId(u64)` | 唯一 Surface 标识 |
| `Surface` | 核心：Terminal + SurfaceSize + SurfaceMouse + KeyboardState + pending_actions |
| `SurfaceSize` | cells 和 pixels 双重尺寸 |
| `SurfaceMouse` / `KeyboardState` | 鼠标位置/拖拽/光标形状 + 按键/IME 状态 |
| `Action` 枚举 | 20+ 变体：窗口/标签/分屏/字体/剪贴板/滚动/搜索/配置重载 |
| `ActionTarget` | `App` 或 `Surface`（决定动作路由） |

关键方法：`key_callback`、`mouse_button_callback`、`cursor_pos_callback`、`scroll_callback`、`size_callback`、`perform_action`。

### split_tree.rs — SplitTree 不可变持久化树（288 行）

使用结构共享的不可变二叉树，管理分割终端面板：

| 类型 | 说明 |
|------|------|
| `PaneId` | `Uuid` 别名 |
| `SplitDirection` | Horizontal / Vertical |
| `SplitNode` | `Leaf { pane_id }` 或 `Split { direction, ratio, first, second }` |
| `SplitTree` | `Option<SplitNode>` 根 + 有序 `Vec<PaneId>` 列表 |

关键方法：`split()`、`remove()`（折叠父级）、`resize()`（0.1-0.9 钳位）、`adjacent()`（焦点移动）、`compute_layout()`（像素矩形计算）。

### tabs/ — 标签页管理

详细文档：[tabs/AGENTS.md](src/tabs/AGENTS.md)

包含：Tab/ClosedTab/TabContent/SplitRef 类型，TabManager 管理标签生命周期、选择切换、恢复关闭标签。

### split/ — 分割面板

详细文档：[split/AGENTS.md](src/split/AGENTS.md)

包含：SplitContainer 递归分割容器（Direction/SplitSide/SplitChild）、四方向拆分算法；layout.rs 布局计算（compute_pane_rects、divider_at_point、DIVIDER_WIDTH）。

### tab_bar.rs — Ghostty 风格标签页栏（GPU 渲染）

原生 GPU 渲染的标签栏，不依赖 egui，直接通过 wgpu 顶点绘制。设计参考 Ghostty 的 NSTabBar 实现。

**核心类型：**

| 类型 | 说明 |
|------|------|
| `TabBarState` | 标签栏渲染状态：visible、hovered_tab_index、hovered_new_tab、hovered_close_tab_index、tab_layouts、new_tab_rect、height、tab_bar_dirty、scale、bar_h、item_h、item_y、close_btn_w、new_btn_w、padding_h |
| `TabLayout` | 单标签布局：tab_id、title、color、rect、close_button_rect |
| `ViewportRect` | 矩形区域（x、y、w、h），用于命中检测 |

**布局常量（逻辑点，运行时乘 scale_factor 转物理像素）：**

| 常量 | 值 | 说明 |
|------|-----|------|
| `TAB_BAR_HEIGHT` | 28.0 | 标签栏高度，与 macOS 红绿灯按钮高度一致（Ghostty 风格） |
| `TAB_ITEM_HEIGHT` | 28.0 | 标签项高度，填满整个标签栏 |
| `TAB_ITEM_Y` | 0.0 | 标签项垂直偏移（无偏移） |
| `TAB_PADDING_H` | 12.0 | 标签水平内边距 |
| `TAB_CLOSE_BTN_WIDTH` | 28.0 | 关闭按钮宽度 |
| `TAB_NEW_BTN_WIDTH` | 36.0 | "+" 新建标签按钮宽度 |
| `TAB_BAR_BORDER_SIZE` | 1.0 | 底部分隔线宽度 |
| `TRAFFIC_LIGHT_WIDTH` | 92.0 (macOS) / 0.0 (其他) | macOS 红绿灯按钮保留宽度（编译时 fallback，运行时通过 API 动态获取） |

**DPI 感知缩放机制：**
1. 所有常量以逻辑点定义，运行时乘以 `window.scale_factor()` 转为物理像素
2. `TabBarState` 缓存缩放后的值（bar_h、item_h 等），供布局和渲染使用
3. `screen_size()` 返回物理像素，布局计算使用缩放后的常量
4. 字体大小同样为 `config.font_size(pt) * scale_factor`

**等分布局算法：**
1. 运行时通过 macOS API 获取红绿灯按钮实际宽度（`traffic_light_width()`）
2. 计算可用宽度 = 窗口物理宽度 - traffic_light_w - TAB_NEW_BTN_WIDTH * scale
3. 每标签等宽 = 可用宽度 / 标签数（无最小宽度限制，严格均分）
4. "+" 按钮固定在窗口右侧

**渲染逻辑（在 main.rs 中实现）：**
- 背景色：终端背景色亮度降低 15%，激活标签使用原始背景色
- 关闭按钮：位于标签左侧，仅悬浮时显示，24pt 缩小背景 + × 居中（`measure_text` 动态居中）
- 关闭按钮 hover：独立的 `hovered_close_tab_index` 追踪，hover 时背景更红
- "+" 按钮：36px，悬浮时高亮，左侧有分隔线

**刷新机制：**
- `tab_bar_dirty` 标志追踪标签栏状态变化（关闭/切换/新建/悬浮）
- `RedrawRequested` early-return 检查包含 `tab_bar_dirty`
- 渲染后重置 dirty 标志

### terminal/ — 终端面板

详细文档：[terminal/AGENTS.md](src/terminal/AGENTS.md)

最核心的组件（1395 行），组合 `Terminal` + `Session`。包含：Selection、SearchState、ScrollState（平滑滚动 + 动量）、TerminalPane，输出处理流程（broadcast → buffer → drain，16 MiB 上限，60fps 限速），完整按键编码表，鼠标事件处理（单击/双击选词/三击选行 + 拖拽 + 平滑滚动），粘贴处理。

**ScrollState** — 平滑滚动：ease-out cubic 动画 + 动量滚动。

**PaneAction** 枚举：CopyText、PasteFromClipboard、ShowContextMenu、OpenUrl、SplitRight/Left/Down/Up、EditTabTitle、EditTabColor。

### command_palette.rs — 命令面板

| 类型 | 说明 |
|------|------|
| `CommandPalette` | 命令面板，支持搜索和执行命令 |
| `Command` | 命令描述符：id、name、category、shortcut |

通过 Ctrl+Shift+P 激活。

### search.rs — 搜索面板

| 类型 | 说明 |
|------|------|
| `SearchPanel` | 搜索面板 UI，含大小写敏感和正则模式 |
| `SlidingWindow` | 跨页搜索滑动窗口（支持 scrollback） |
| `ViewportSearch` | 可视区域搜索（指纹变更检测） |

### focus_mode.rs — 专注模式（76 行）

隐藏所有 UI 元素（标签栏、标题栏、侧边栏），仅显示终端内容。记忆进入前的可见状态以便恢复。

### jump_prompt.rs — 跳转提示（74 行）

基于 OSC 133 语义 prompt mark 的提示间跳转：

| 方法 | 说明 |
|------|------|
| `next_prompt(current_row, marks)` | 跳到下一个 prompt mark |
| `prev_prompt(current_row, marks)` | 跳到上一个 prompt mark |
| `get_command_at(row, marks, rows)` | 提取 prompt 处的命令文本 |

### online_search.rs — 在线搜索面板（135 行）

egui 侧边栏，支持选中文本后在线搜索。包含搜索输入、加载动画、错误显示和可滚动结果列表。

| 类型 | 说明 |
|------|------|
| `OnlineSearchPanel` | visible、query、results、loading、error |
| `SearchEntry` | title、url、snippet |

### quick_bar.rs — 快速命令栏（116 行）

终端顶部的浮动内联命令栏，三种模式：

| 模式 | 说明 |
|------|------|
| `Command` | 快速命令执行 |
| `Search` | 搜索 |
| `Filter` | 过滤 |

### ai_chat.rs — AI 聊天面板

集成 AI 对话功能，可在侧边栏显示 AI 助手对话。

### context_menu.rs — 右键菜单

`MenuAction` 枚举（17 个变体）：Copy、Paste、SelectAll、Clear、Search、SplitRight/Left/Down/Up、ClosePane、NewTab、CloseTab、ZoomIn/Out/Reset、OpenSettings、EditTabTitle、EditTabColor。

### inspector.rs — 终端检查器

调试面板：终端状态（光标、尺寸、模式）、渲染器信息（FPS、纹理内存）、键盘事件历史、VT 解析日志。

### explorer.rs — 文件浏览器

集成文件浏览器面板，支持本地和 SFTP 远程文件浏览。

### clipboard.rs — 剪贴板管理

`ClipboardManager` 系统剪贴板集成。

### tab_recovery.rs — 标签页恢复

`TabRecovery` / `SavedTab` / `SavedWindowState` 保存和恢复标签页布局。

### lock_screen.rs — 锁屏

终端锁屏功能，输入密码解锁。

### theme.rs — 主题管理

### tokens.rs — UI token 定义

### welcome.rs — 欢迎页面

## 常见任务

### 添加新的快捷键处理
1. 在 `pureterm-core` 的 `hotkey.rs` 添加快捷键定义
2. 在 `app.rs` 的 `handle_key` 中添加匹配和处理逻辑

### 添加新的 Action（Surface 动作）
1. 在 `surface.rs` 的 `Action` 枚举中添加变体
2. 在 `Action::target()` 中分类为 App 或 Surface 级别
3. 在 `Surface::handle_surface_action` 或 `handle_app_action` 中实现处理逻辑

### 修改标签页行为
详细文档：[tabs/AGENTS.md](src/tabs/AGENTS.md)

### 修改分割面板逻辑
详细文档：[split/AGENTS.md](src/split/AGENTS.md)

### 修改按键映射
详细文档：[terminal/AGENTS.md](src/terminal/AGENTS.md)

### 添加新的 UI 组件
1. 在 `src/` 下创建模块文件
2. 在 `lib.rs` 中导出
3. 在 `app.rs` 中集成到应用状态
