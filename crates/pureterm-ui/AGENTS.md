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
├── jump_prompt.rs       # 跳转提示（Ctrl+;）
├── lock_screen.rs       # 锁屏
├── online_search.rs     # 在线搜索
├── quick_bar.rs         # 快速命令栏
├── search.rs            # 搜索面板
├── settings/            # 设置面板
│   └── mod.rs
├── split/
│   ├── mod.rs           # 重导出（含 SplitSide、DividerInfo、divider_at_point 等）
│   ├── container.rs     # SplitContainer + SplitChild + Direction + SplitSide
│   └── layout.rs        # compute_pane_rects、compute_divider_rects、divider_at_point、PaneRect
├── tabs/
│   ├── mod.rs           # 重导出
│   └── manager.rs       # TabManager + Tab + TabContent + ClosedTab
└── terminal/
    ├── mod.rs           # 重导出
    └── pane.rs          # TerminalPane + Selection + PaneAction（含 4 方向 Split）
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

### tabs/manager.rs — 标签页管理

| 类型 | 说明 |
|------|------|
| `Tab` | id、title、icon、color、content、has_activity、has_focus、created_at |
| `TabContent` | `Pane(u64)` 或 `Split(SplitRef)` |
| `ClosedTab` | 已关闭标签的恢复信息 |
| `TabManager` | 管理 tabs 列表、active_tab、closed_tabs_stack |

`TabManager` 方法：`open_tab`、`close_tab`、`select_tab`、`previous_tab`/`next_tab`、`restore_last_closed`、`set_title`、`mark_activity`。

### tab_recovery.rs — 标签页恢复

| 类型 | 说明 |
|------|------|
| `TabRecovery` | 标签页恢复服务 |
| `SavedTab` | 保存的标签状态 |
| `SavedWindowState` | 保存的窗口状态 |

保存和恢复标签页布局。

### split/container.rs — 分割面板

| 类型 | 说明 |
|------|------|
| `Direction` | Horizontal / Vertical |
| `SplitSide` | Right / Left / Down / Up — 四方向拆分，提供 `direction()` 和 `insert_before()` 方法 |
| `SplitChild` | `Container(Box<SplitContainer>)` 或 `Pane(u64)` |
| `SplitContainer` | direction、children、ratios、id |

关键方法：
- `split_pane(pane_id, new_pane_id, direction)` — 递归分割；同方向插入相邻，垂直方向创建嵌套容器
- `split_pane_directional(pane_id, new_pane_id, side: SplitSide)` — 四方向分割：Right/Down 在目标后插入，Left/Up 在目标前插入；垂直方向时创建嵌套容器
- `close_pane(pane_id)` — 递归关闭，自动清理空容器
- `equalize()` — 递归等分所有比例
- `adjust_ratio(index, delta)` — 调整分割比例（用于拖拽）
- `find_container_mut(id)` — 递归查找指定 ID 的容器（用于拖拽时 adjust_ratio）
- `pane_ids()` / `pane_count()` — 获取所有窗格 ID 和数量

### split/layout.rs — 布局计算

| 常量/类型 | 说明 |
|------|------|
| `DIVIDER_WIDTH = 4.0` | 可见分隔线宽度（像素） |
| `DIVIDER_HIT_WIDTH = 8.0` | 分隔线命中区域宽度（像素，用于拖拽检测） |
| `PaneRect` | 窗格矩形：x、y、w、h |
| `DividerInfo` | 分隔线信息：container_id、child_index、direction、rect |

关键函数：
- `compute_pane_rects(container, x, y, w, h)` — 递归计算所有窗格的像素矩形
- `compute_divider_rects(container, x, y, w, h)` — 递归计算所有分隔线的矩形
- `divider_at_point(container, px, py, x, y, w, h)` — 检测鼠标是否在分隔线上（扩展命中区域）
- `pane_at_point(layout, px, py)` — 检测鼠标在哪个窗格上

### terminal/pane.rs — 终端面板

最核心的组件，组合 `Terminal` + `Session`。

| 类型 | 说明 |
|------|------|
| `Selection` | start_row/col、end_row/col、is_block |
| `SearchState` | query、matches、current_match、case_sensitive、regex |
| `TerminalPane` | terminal + session + selection + search + zoom + output_buffer |
| `PaneAction` | 窗格动作枚举（含 SplitRight/Left/Down/Up、CopyText、PasteFromClipboard、ShowContextMenu、OpenUrl、EditTabTitle、EditTabColor） |

`TerminalPane` 关键方法：

| 方法 | 说明 |
|------|------|
| `new(cols, rows, scrollback)` | 创建面板 |
| `set_session(session)` | 设置会话并启动输出处理 |
| `set_session_with_receiver(session, receiver)` | 使用预创建的接收器设置会话 |
| `drain_output()` | 从 output_buffer 读取数据并写入 terminal |
| `write_input(data)` | 写入 session |
| `handle_key(key, ctrl, alt, shift)` | 编码按键为转义序列并写入 session |
| `encode_key(key, ctrl, alt, shift)` | 按键到转义序列的映射 |
| `resize(cols, rows)` | 调整 terminal 和 session |
| `copy_selection()` / `paste(text)` | 剪贴板操作 |
| `zoom_in/out/reset()` | 缩放（范围 0.5-3.0） |
| `update_search()` | 搜索可见文本 |
| `handle_mouse_event(event)` | 处理鼠标事件，返回待发送数据和 PaneAction |
| `url_at(row, col)` | 获取指定位置的超链接 |
| `pixel_to_cell(px, py, cell_w, cell_h)` | 像素坐标转单元格坐标 |

**输出处理流程**：
`Session` → `broadcast channel` → 后台线程（`start_output_processing`）→ `output_buffer` → `drain_output()` → `Terminal::write()`

**按键编码**：Ctrl+A-Z → 0x01-0x1A，特殊键（Enter、Backspace、Tab、Escape、方向键等）→ 对应转义序列，Shift 修饰键处理，普通字符透传。

### command_palette.rs — 命令面板

| 类型 | 说明 |
|------|------|
| `CommandPalette` | 命令面板，支持搜索和执行命令 |
| `Command` | 命令描述符：id、name、category、shortcut |

通过 Ctrl+Shift+P 激活。包含 split_right、split_down、split_left、split_up 命令。

### search.rs — 搜索面板

| 类型 | 说明 |
|------|------|
| `SearchPanel` | 搜索面板 UI |
| `SearchOptions` | 搜索选项：大小写敏感、正则、整词 |

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

集成文件浏览器面板。

### focus_mode.rs — 专注模式

隐藏所有 UI 元素，仅显示终端内容。

### lock_screen.rs — 锁屏

终端锁屏功能，输入密码解锁。

### online_search.rs — 在线搜索

选中文本后在线搜索功能。

### quick_bar.rs — 快速命令栏

快速执行命令的浮动面板。

## 常见任务

### 添加新的快捷键处理
1. 在 `pureterm-core` 的 `hotkey.rs` 添加快捷键定义
2. 在 `app.rs` 的 `handle_key` 中添加匹配和处理逻辑

### 修改标签页行为
编辑 `tabs/manager.rs`。`open_tab` 和 `close_tab` 管理标签生命周期。

### 修改分割面板逻辑
编辑 `split/container.rs`。注意 `split_pane_directional` 的递归逻辑 — 同方向在容器内插入，垂直方向创建嵌套。`close_pane` 会自动清理空容器。

### 添加新的拆分方向
1. 在 `SplitSide` 枚举中添加变体
2. 实现 `direction()` 和 `insert_before()` 方法
3. 在 `split_pane_directional` 中处理新方向
4. 更新 `context_menu.rs` 的 `MenuAction` 和菜单项
5. 更新 `pane.rs` 的 `PaneAction`

### 修改按键映射
编辑 `terminal/pane.rs` 的 `encode_key` 方法。特殊键通过 match 映射到转义序列。

### 添加新的 UI 组件
1. 在 `src/` 下创建模块目录
2. 在 `lib.rs` 中导出
3. 在 `app.rs` 中集成到应用状态

### 处理鼠标事件
`TerminalPane::handle_mouse_event()` 返回需要发送给终端的数据和 PaneAction。

### 修改分隔线布局
编辑 `split/layout.rs`。`DIVIDER_WIDTH` 控制可见宽度，`DIVIDER_HIT_WIDTH` 控制可拖拽区域。`compute_divider_rects` 和 `divider_at_point` 使用相同的递归遍历逻辑。
