# pureterm-ui — egui 组件层

基于 egui 的 UI 组件：应用状态管理、标签页、分割面板、终端面板、命令面板、搜索、AI 聊天等。

## 关键依赖

内部依赖：`pureterm-core`、`pureterm-terminal`、`pureterm-session`

外部依赖：`egui` + `epaint`（UI）、`tokio`（异步）、`serde`、`thiserror`、`parking_lot`。

## 文件结构

```
src/
├── lib.rs              # 模块导出
├── app.rs              # App 主应用状态
├── ai_chat.rs          # AI 聊天面板
├── autocomplete.rs      # 自动补全
├── clipboard.rs         # 剪贴板管理
├── command_palette.rs   # 命令面板（Ctrl+Shift+P）
├── context_menu.rs      # 右键菜单（17 个 MenuAction）
├── explorer.rs          # 文件浏览器
├── focus_mode.rs        # 专注模式
├── inspector.rs         # 终端检查器（调试面板）
├── jump_prompt.rs       # 跳转提示（Ctrl+;）
├── lock_screen.rs       # 锁屏
├── online_search.rs     # 在线搜索
├── profile_selector.rs  # 连接配置选择器
├── quick_bar.rs         # 快速命令栏
├── search.rs            # 搜索面板（SlidingWindow + ViewportSearch）
├── settings/
│   ├── mod.rs          # 设置面板
│   └── AGENTS.md       # 设置面板详解
├── split/
│   ├── mod.rs           # 重导出（含 SplitSide、DividerInfo、divider_at_point 等）
│   ├── container.rs     # SplitContainer + SplitChild + Direction + SplitSide
│   ├── layout.rs        # compute_pane_rects、compute_divider_rects、divider_at_point、PaneRect
│   ├── split_tree.rs    # 分割树管理
│   └── AGENTS.md        # 分割面板布局详解
├── surface.rs           # Surface 表面管理
├── tab_bar.rs           # 标签页栏 UI 组件
├── tab_recovery.rs      # 标签页恢复
├── tabs/
│   ├── mod.rs           # 重导出
│   ├── manager.rs       # TabManager + Tab + TabContent + ClosedTab
│   └── AGENTS.md        # 标签页管理详解
├── terminal/
│   ├── mod.rs           # 重导出
│   ├── pane.rs          # TerminalPane + Selection + PaneAction（含 4 方向 Split）
│   └── AGENTS.md        # 终端面板详解
├── theme.rs             # 主题管理
├── tokens.rs            # UI token 定义
└── welcome.rs           # 欢迎页面
```

## 模块详情

### app.rs — 主应用状态

| 字段 | 类型 | 说明 |
|------|------|------|
| `config` | `AppConfig` | 应用配置 |
| `tab_manager` | `TabManager` | 标签页管理器 |
| `hotkey_service` | `HotkeyService` | 快捷键服务 |
| `show_settings` | `bool` | 设置面板可见性 |
| `search_visible` | `bool` | 搜索栏可见性 |
| `search_query` | `String` | 搜索查询 |
| `quitting` | `bool` | 退出标记 |

`handle_key(key, ctrl, alt, shift) -> bool` — 检查快捷键绑定，处理 tab_new、tab_close、search、prev_tab、next_tab。

### tabs/ — 标签页管理

详细文档：[tabs/AGENTS.md](src/tabs/AGENTS.md)

包含：Tab/ClosedTab/TabContent/SplitRef 类型，TabManager 管理标签生命周期、选择切换、恢复关闭标签。

### tab_recovery.rs — 标签页恢复

| 类型 | 说明 |
|------|------|
| `TabRecovery` | 标签页恢复服务 |
| `SavedTab` | 保存的标签状态 |
| `SavedWindowState` | 保存的窗口状态 |

保存和恢复标签页布局。

### split/ — 分割面板

详细文档：[split/AGENTS.md](src/split/AGENTS.md)

包含：SplitContainer 递归分割容器（Direction/SplitSide/SplitChild）、四方向拆分算法；layout.rs 布局计算（compute_pane_rects、divider_at_point、DIVIDER_WIDTH）。

### terminal/ — 终端面板

详细文档：[terminal/AGENTS.md](src/terminal/AGENTS.md)

最核心的组件，组合 `Terminal` + `Session`。包含 Selection/SearchState/TerminalPane 类型，输出处理流程（broadcast → buffer → drain），完整按键编码表（Ctrl/Alt/Shift 组合），鼠标事件处理，粘贴处理，右键行为。

### command_palette.rs — 命令面板

| 类型 | 说明 |
|------|------|
| `CommandPalette` | 命令面板，支持搜索和执行命令 |
| `Command` | 命令描述符：id、name、category、shortcut |

通过 Ctrl+Shift+P 激活。包含 split_right、split_down、split_left、split_up 命令。

### search.rs — 搜索面板

| 类型 | 说明 |
|------|------|
| `SearchPanel` | 搜索面板 UI，含大小写敏感和正则模式 |
| `SearchConfig` | 搜索配置：case_sensitive、regex_mode、direction |
| `SearchThread` | 后台搜索线程（mpsc 通道通信） |
| `SlidingWindow` | 跨页搜索滑动窗口（支持 scrollback） |
| `ViewportSearch` | 可视区域搜索（指纹变更检测） |

### ai_chat.rs — AI 聊天面板

集成 AI 对话功能，可在侧边栏显示 AI 助手对话。

### autocomplete.rs — 自动补全

终端自动补全功能，支持上下文感知的命令和文件名补全。

### context_menu.rs — 右键菜单

| 类型 | 说明 |
|------|------|
| `ContextMenuState` | 右键上下文菜单状态（位置、悬浮项、可见性） |
| `MenuAction` | 菜单动作枚举（17 个变体） |

`MenuAction` 变体：Copy、Paste、SelectAll、Clear、Search、SplitRight (Ctrl+Shift+D)、SplitLeft、SplitDown (Ctrl+D)、SplitUp、ClosePane、NewTab、CloseTab、ZoomIn、ZoomOut、ZoomReset、OpenSettings、EditTabTitle、EditTabColor。

### jump_prompt.rs — 跳转提示

Ctrl+; 激活的快速跳转面板，可跳转到指定行号或匹配项。

### clipboard.rs — 剪贴板管理

| 类型 | 说明 |
|------|------|
| `ClipboardManager` | 系统剪贴板集成 |

### explorer.rs — 文件浏览器

集成文件浏览器面板，支持本地和 SFTP 远程文件浏览。

| 方法 | 说明 |
|------|------|
| `set_remote_entries(entries)` | 设置远程文件列表（SFTP） |
| `set_remote_error(msg)` | 显示远程错误 |
| `set_loading(loading)` | 设置加载状态 |

### focus_mode.rs — 专注模式

隐藏所有 UI 元素，仅显示终端内容。

### lock_screen.rs — 锁屏

终端锁屏功能，输入密码解锁。

### inspector.rs — 终端检查器

调试面板，用于查看终端状态、渲染器信息、键盘事件和 VT 解析日志：

| 类型 | 说明 |
|------|------|
| `InspectorState` | 检查器状态，包含可见性、终端信息、渲染器信息 |
| `TerminalDebugInfo` | 终端调试信息：光标位置、屏幕尺寸、模式标志、标题、目录 |
| `RendererDebugInfo` | 渲染器调试信息：帧计数、FPS、纹理内存、绘制调用 |
| `KeyEventRecord` | 键盘事件历史记录：时间戳、按键、修饰键、是否消费 |

**InspectorState 方法：**
- `toggle()` — 切换可见性
- `is_visible()` / `set_visible(bool)` — 检查/设置可见性
- `update_terminal(info)` — 更新终端调试信息
- `update_renderer(info)` — 更新渲染器调试信息
- `record_key(key, mods, consumed)` — 记录键盘事件
- `log_vt(event)` — 记录 VT 事件
- `draw(ui)` — 绘制检查器 UI（包含 4 个可折叠部分）

### online_search.rs — 在线搜索

选中文本后在线搜索功能。

### quick_bar.rs — 快速命令栏

快速执行命令的浮动面板。

## 常见任务

### 添加新的快捷键处理
1. 在 `pureterm-core` 的 `hotkey.rs` 添加快捷键定义
2. 在 `app.rs` 的 `handle_key` 中添加匹配和处理逻辑

### 修改标签页行为
详细文档：[tabs/AGENTS.md](src/tabs/AGENTS.md)

### 修改分割面板逻辑
详细文档：[split/AGENTS.md](src/split/AGENTS.md)

### 添加新的拆分方向
详细文档：[split/AGENTS.md](src/split/AGENTS.md) — 在 `SplitSide` 枚举中添加变体，实现 `direction()` 和 `insert_before()` 方法，更新 context_menu 和 pane 的相关枚举。

### 修改按键映射
详细文档：[terminal/AGENTS.md](src/terminal/AGENTS.md) — 编辑 `encode_key` 方法。

### 添加新的 UI 组件
1. 在 `src/` 下创建模块目录
2. 在 `lib.rs` 中导出
3. 在 `app.rs` 中集成到应用状态

### 处理鼠标事件
`TerminalPane::handle_mouse_event()` 返回需要发送给终端的数据和 PaneAction。详细文档：[terminal/AGENTS.md](src/terminal/AGENTS.md)

### 修改分隔线布局
详细文档：[split/AGENTS.md](src/split/AGENTS.md) — `DIVIDER_WIDTH` 控制可见宽度，`DIVIDER_HIT_WIDTH` 控制可拖拽区域。
