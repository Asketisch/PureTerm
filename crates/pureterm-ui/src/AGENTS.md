# pureterm-ui/src — UI 组件层源码

基于 egui 的 UI 组件全部源文件：应用状态管理、Surface 抽象、标签页、分割面板、终端面板、命令面板、搜索、AI 聊天、检查器、设置等。

← [pureterm-ui/AGENTS.md](../AGENTS.md)

## 文件结构

```
src/
├── lib.rs               # crate 入口，re-export 所有 UI 类型 (47 行)
├── app.rs               # App 状态机 + AppMessage 消息循环 (177 行)
├── ai_chat.rs           # AI 聊天面板 (159 行)
├── autocomplete.rs      # 命令自动补全弹出窗口 (301 行)
├── clipboard.rs         # ClipboardManager 剪贴板管理 (57 行)
├── command_palette.rs   # 命令面板 Ctrl+Shift+P (182 行)
├── context_menu.rs      # 右键上下文菜单（17 个 MenuAction）(235 行)
├── explorer.rs          # 文件浏览器面板（本地 + SFTP）(207 行)
├── focus_mode.rs        # 专注模式 — 隐藏所有 UI (75 行)
├── history_menu.rs      # 命令历史菜单 VecDeque MAX_HISTORY=10000 (279 行)
├── inspector.rs         # 终端调试检查器面板 (776 行)
├── jump_prompt.rs       # 跳转提示 Ctrl+; OSC 133 (73 行)
├── lock_screen.rs       # 锁屏 — 密码保护 (146 行)
├── online_search.rs     # 在线搜索面板 (134 行)
├── profile_selector.rs  # 配置文件选择器 (359 行)
├── quick_bar.rs         # 快速操作栏 Command/Search/Filter (115 行)
├── search.rs            # 搜索面板 SlidingWindow + ViewportSearch (625 行)
├── split_tree.rs        # SplitTree 不可变持久化二叉树 (287 行)
├── surface.rs           # Surface 抽象 Ghostty 风格 Action 消息 (582 行)
├── tab_bar.rs           # 标签栏 GPU 渲染 TabBarState (114 行)
├── tab_recovery.rs      # 标签页恢复持久化 (79 行)
├── theme.rs             # 主题应用和配色 (306 行)
├── tokens.rs            # UI 设计令牌 间距/圆角/笔画 (76 行)
├── welcome.rs           # 欢迎页面 (285 行)
├── settings/            → settings/AGENTS.md — 设置面板
├── split/               → split/AGENTS.md — 分割面板布局
├── tabs/                → tabs/AGENTS.md — 标签页管理
└── terminal/            → terminal/AGENTS.md — 终端面板
```

## 模块详情

### lib.rs — crate 入口

声明所有子模块，重导出核心类型：App、ClipboardManager、CommandPalette、ContextMenu、InspectorState、ProfileSelector、SettingsPanel、SplitContainer、Surface/SurfaceId/Action、TabManager、TerminalPane、ColorTheme、WelcomePage 等。

### app.rs — 应用状态机（177 行）

Ghostty `App.zig` 风格的应用状态管理，基于邮箱消息循环。

| 字段 | 说明 |
|------|------|
| `config` | AppConfig 应用配置 |
| `tab_manager` | TabManager 标签页管理器 |
| `hotkey_service` | HotkeyService 快捷键服务 |
| `surfaces` | Vec<SurfaceId> Surface 注册表 |
| `focused_surface` | Option<SurfaceId> 当前聚焦 Surface |
| `mailbox` | Vec<AppMessage> 消息队列 |

**AppMessage**（15 个变体）：Quit、NewWindow、NewTab、CloseTab、NewSplit、ToggleFullscreen、ToggleMaximize、MoveTab、GotoTab、ResizeSplit、EqualizeSplits、MoveFocus、ReloadConfig、ToggleInspector、ToggleSearch、ConfigChanged、SurfaceDestroyed。

`handle_key()` 方法检查快捷键绑定并分发消息。

### ai_chat.rs — AI 聊天面板

| 类型 | 说明 |
|------|------|
| `AiChatPanel` | 聊天面板状态：visible、messages、input、loading |
| `ChatMessage` | 消息：role（User/Assistant/System）、content、timestamp |
| `ChatRole` | User / Assistant / System |

egui 侧边栏 AI 对话界面，显示消息历史和输入框。

### autocomplete.rs — 命令自动补全（301 行）

| 类型 | 说明 |
|------|------|
| `CompletionCandidate` | 候选项：text、kind、description |
| `CompletionKind` | Command / File / Directory / Variable / Hostname / History / SshProfile |
| `AutoComplete` | 状态：visible、candidates、selected_index、position |

基于当前输入上下文提供补全建议，支持键盘导航。

### clipboard.rs — 剪贴板管理

| 类型 | 说明 |
|------|------|
| `ClipboardManager` | 剪贴板操作：copy、paste、last_written、has_content |

记录最后写入内容，粘贴时返回最后写入内容作为后备。

### command_palette.rs — 命令面板（182 行）

| 类型 | 说明 |
|------|------|
| `CommandEntry` | 命令条目：id、label、category、shortcut |
| `CommandPalette` | 状态：visible、query、selected_index、commands |

通过 Ctrl+Shift+P 激活，提供可搜索的命令列表，包含内置命令集合。

### context_menu.rs — 右键上下文菜单（235 行）

| 类型 | 说明 |
|------|------|
| `MenuAction` | 17 种动作：Copy、Paste、SelectAll、Clear、Search、SplitRight/Left/Down/Up、ClosePane、NewTab、CloseTab、ZoomIn/Out/Reset、OpenSettings、EditTabTitle、EditTabColor |
| `MenuItem` | 菜单项：label、action、enabled、separator_before |
| `ContextMenuState` | 菜单状态：visible、position、hovered_index、items |

不依赖 egui 原生实现，直接通过 GPU 渲染。导出菜单常量：MENU_ITEM_HEIGHT、MENU_MIN_WIDTH、MENU_PADDING_H/V、MENU_SEPARATOR_HEIGHT。

### explorer.rs — 文件浏览器

| 类型 | 说明 |
|------|------|
| `FileEntry` | 文件/目录：name、path、is_dir、size、modified |
| `ExplorerPanel` | 浏览器面板：visible、current_path、entries、selected_index、loading、error、is_remote |

支持本地文件系统和 SFTP 远程浏览。

### focus_mode.rs — 专注模式

| 类型 | 说明 |
|------|------|
| `FocusMode` | 状态：active、was_tab_bar_visible、was_title_bar_visible、was_sidebar_visible |

`toggle()` 切换专注模式，记忆进入前的 UI 可见状态以便恢复。隐藏标签栏、标题栏和侧边栏，仅显示终端内容。

### history_menu.rs — 命令历史菜单（279 行）

| 类型 | 说明 |
|------|------|
| `CommandHistory` | 命令历史：entries（VecDeque，MAX_HISTORY=10000）、query、filtered、selected |

- `push(command)` — 添加命令（跳过空行和最近重复）
- `search(query)` — 前缀搜索过滤
- `select_next()` / `select_prev()` — 键盘导航

### inspector.rs — 终端调试检查器（776 行）

| 类型 | 说明 |
|------|------|
| `TerminalDebugInfo` | 终端状态：cursor_x/y、cols/rows、mode_flags、title、cwd |
| `RendererDebugInfo` | 渲染器：frame_count、fps、texture_memory、draw_calls |
| `GridDebugInfo` | 网格：rows/cols、scrollback_size、scroll_offset、cursor_row |
| `ModesDebugInfo` | 模式：enabled_modes 列表 |
| `FontAtlasDebugInfo` | 字体图集：atlas_size、glyph_count、cache_hit_rate |
| `PerfDebugInfo` | 性能：frame_time_ms、input_latency_ms |
| `ConfigDebugInfo` | 配置：active_theme、font_family、font_size |
| `InspectorState` | 检查器总状态 |

egui 面板，使用 CollapsingHeader 组织各调试类别。

### jump_prompt.rs — 跳转提示

| 类型 | 说明 |
|------|------|
| `JumpToPrompt` | 管理提示标记间导航：current_index |

- `next_prompt(current_row, marks)` — 跳到下一个 prompt mark
- `prev_prompt(current_row, marks)` — 跳到上一个 prompt mark
- `get_command_at(row, marks, rows)` — 提取 prompt 处的命令文本

### lock_screen.rs — 锁屏

| 类型 | 说明 |
|------|------|
| `LockScreen` | 锁屏状态：active、password_hash、input、shake_offset、failed_attempts |

- `set_password()` — 使用 DefaultHasher 设置密码哈希
- `lock()` / `unlock()` — 锁定/解锁终端
- 输入错误密码时显示抖动动画效果

### online_search.rs — 在线搜索面板

| 类型 | 说明 |
|------|------|
| `OnlineSearchPanel` | 面板：visible、query、results、loading、error |
| `SearchEntry` | 结果：title、url、snippet |
| `OnlineSearchAction` | 动作：OpenUrl、Close |

egui 侧边栏，支持选中文本后在线搜索。

### profile_selector.rs — 配置文件选择器（359 行）

| 类型 | 说明 |
|------|------|
| `ProfileSelector` | 选择器：visible、query、selected_index |

支持快速连接（`user@host`、`host:port` 等格式），从 AppConfig 的 profiles 列表中筛选匹配项。Escape 关闭，Enter 确认选择。

### quick_bar.rs — 快速操作栏

| 类型 | 说明 |
|------|------|
| `QuickBar` | 状态：visible、query、mode |
| `QuickBarMode` | Command / Search / Filter |
| `QuickBarAction` | Submit(query)、Close |

终端顶部的浮动内联输入栏，三种工作模式。

### search.rs — 搜索面板（625 行）

| 类型 | 说明 |
|------|------|
| `SearchPanel` | 搜索面板 UI |
| `SlidingWindow` | 跨页搜索滑动窗口（支持 scrollback） |
| `ViewportSearch` | 可视区域搜索（指纹变更检测） |
| `SearchMatch` | 搜索结果：grid_pos + text byte range |
| `SearchMode` | Viewport / Scrollback / All |

后台线程搜索，基于 Ghostty 架构设计。

### split_tree.rs — 不可变持久化分割树（287 行）

| 类型 | 说明 |
|------|------|
| `PaneId` | `Uuid` 别名 |
| `SplitDirection` | Horizontal / Vertical |
| `SplitNode` | Leaf{pane_id} 或 Split{direction, ratio, first, second} |
| `SplitTree` | Option<SplitNode> 根 + 有序 Vec<PaneId> |

使用 `im` crate 实现结构共享。关键方法：`split()`、`remove()`（折叠父级）、`resize()`（0.1-0.9 钳位）、`adjacent()`（焦点移动）、`compute_layout()`（像素矩形计算）。

### surface.rs — Surface 抽象（582 行）

Ghostty 风格的单终端 Surface 模型。

| 类型 | 说明 |
|------|------|
| `SurfaceId(u64)` | 唯一标识 |
| `Surface` | 核心：Terminal + SurfaceSize + SurfaceMouse + KeyboardState + pending_actions |
| `SurfaceSize` | cells 和 pixels 双重尺寸 |
| `SurfaceMouse` / `KeyboardState` | 鼠标/键盘状态 |
| `Action` 枚举 | 20+ 变体：窗口/标签/分屏/字体/剪贴板/滚动/搜索 |
| `ActionTarget` | App 或 Surface（决定动作路由） |
| `CursorShape` | 光标形状 |
| `SplitDirection` / `SplitResizeParameter` | 分屏参数 |

关键方法：`key_callback`、`mouse_button_callback`、`cursor_pos_callback`、`scroll_callback`、`size_callback`、`perform_action`。

### tab_bar.rs — 标签栏 GPU 渲染

| 类型 | 说明 |
|------|------|
| `TabBarState` | 渲染状态：visible、hovered_tab_index、hovered_new_tab、hovered_close_tab_index、tab_layouts、new_tab_rect、height、tab_bar_dirty、scale + 缩放缓存字段 |
| `TabLayout` | 单标签布局：tab_id、title、color、rect、close_button_rect |
| `ViewportRect` | 矩形区域（x、y、w、h） |

原生 GPU 渲染，不依赖 egui。所有尺寸以逻辑点定义，运行时乘 scale_factor。28pt 高度与 macOS 红绿灯按钮一致。

### tab_recovery.rs — 标签页恢复

| 类型 | 说明 |
|------|------|
| `SavedTab` | 序列化标签页：title、session_type、profile_name、connection_info、working_dir |
| `SavedWindowState` | 窗口状态：tabs、active_tab_index、window_width/height |
| `TabRecovery` | 持久化管理：save/tabs/restore |

持久化路径 `~/.config/pureterm/tab_state.json`。

### theme.rs — 主题系统（306 行）

| 函数/类型 | 说明 |
|------|------|
| `ColorTheme` | 语义颜色主题结构 |
| `apply_theme(ctx, theme)` | 应用主题到 egui Context |
| `set_theme()` | 设置 Visuals |
| `dark_mode()` / `light_mode()` | 获取深色/浅色 Visuals |
| `dark_color_theme()` / `light_color_theme()` | 获取对应主题 |

定义强调色（PURPLE）、背景色层级、灰色层级等设计令牌。

### tokens.rs — UI 设计令牌

集中定义间距、圆角、笔画宽度：

| 常量类别 | 值 |
|------|-----|
| 间距 | XS=4, SM=8, MD=12, LG=16, XL=24, XXL=32 (px) |
| 圆角 | SM=4, MD=8, LG=12, PILL=18 (px) |
| 笔画 | THIN=1.0, MEDIUM=1.5, THICK=2.0 (px) |

### welcome.rs — 欢迎页面（285 行）

| 类型 | 说明 |
|------|------|
| `WelcomePage` | 欢迎页：visible、quick_connect、selected_profile |

首次启动时显示，提供快速连接输入和配置文件选择列表。

## 子模块链接

- [settings/AGENTS.md](settings/AGENTS.md) — 设置面板（外观、通用、快捷键、配置文件）
- [split/AGENTS.md](split/AGENTS.md) — 分割面板布局（SplitContainer、compute_pane_rects）
- [tabs/AGENTS.md](tabs/AGENTS.md) — 标签页管理（TabManager、Tab、TabContent）
- [terminal/AGENTS.md](terminal/AGENTS.md) — 终端面板（TerminalPane、Selection、ScrollState）

## 常见任务

### 添加新的 UI 组件
1. 在 `src/` 下创建模块文件
2. 在 `lib.rs` 中添加 `pub mod` 和 `pub use` 导出
3. 在 `app.rs` 中集成到应用状态
4. 在渲染循环中调用组件的 show/render 方法

### 添加新的快捷键处理
1. 在 `pureterm-core` 的 `hotkey.rs` 添加快捷键定义
2. 在 `app.rs` 的 `handle_key` 中添加匹配和处理逻辑

### 添加新的 Action
1. 在 `surface.rs` 的 `Action` 枚举中添加变体
2. 在 `Action::target()` 中分类为 App 或 Surface 级别
3. 在对应 handler 中实现处理逻辑

### 修改标签页行为
参考 [tabs/AGENTS.md](tabs/AGENTS.md)

### 修改分割面板逻辑
参考 [split/AGENTS.md](split/AGENTS.md)

### 修改按键映射
参考 [terminal/AGENTS.md](terminal/AGENTS.md)

### 添加新的命令面板条目
在 `command_palette.rs` 的 `built_in_commands()` 中添加新的 `CommandEntry`。
