# pureterm-ui — egui 组件层

基于 egui 的 UI 组件：应用状态管理、标签页、分割面板、终端面板。

## 关键依赖

内部依赖：`pureterm-core`、`pureterm-terminal`、`pureterm-session`

外部依赖：`egui` + `epaint`（UI）、`tokio`（异步）、`serde`、`thiserror`、`parking_lot`。

## 文件结构

```
src/
├── lib.rs           # 模块导出
├── app.rs           # App 主应用状态
├── tabs/
│   ├── mod.rs       # 重导出
│   └── manager.rs   # TabManager + Tab + TabContent + ClosedTab
├── split/
│   ├── mod.rs       # 重导出
│   └── container.rs # SplitContainer + SplitChild + Direction
├── terminal/
│   ├── mod.rs       # 重导出
│   └── pane.rs      # TerminalPane + Selection + SearchState
└── settings/
    └── mod.rs       # 占位模块（设置 UI 面板，待扩展）
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

### split/container.rs — 分割面板

| 类型 | 说明 |
|------|------|
| `Direction` | Horizontal / Vertical |
| `SplitChild` | `Container(Box<SplitContainer>)` 或 `Pane(u64)` |
| `SplitContainer` | direction、children、ratios、id |

关键方法：
- `split_pane(pane_id, new_pane_id, direction)` — 递归分割；同方向插入相邻，垂直方向创建嵌套容器
- `close_pane(pane_id)` — 递归关闭，自动清理空容器
- `equalize()` — 等分所有比例
- `adjust_ratio(index, delta)` — 调整分割比例

### terminal/pane.rs — 终端面板

最核心的组件，组合 `Terminal` + `Session`。

| 类型 | 说明 |
|------|------|
| `Selection` | start_row/col、end_row/col、is_block |
| `SearchState` | query、matches、current_match、case_sensitive、regex |
| `TerminalPane` | terminal + session + selection + search + zoom + output_buffer |

`TerminalPane` 关键方法：

| 方法 | 说明 |
|------|------|
| `new(cols, rows, scrollback)` | 创建面板 |
| `set_session(session)` | 设置会话并启动输出处理 |
| `drain_output()` | 从 output_buffer 读取数据并写入 terminal |
| `write_input(data)` | 写入 session |
| `handle_key(key, ctrl, alt, shift)` | 编码按键为转义序列并写入 session |
| `encode_key(key, ctrl, alt, shift)` | 按键到转义序列的映射 |
| `resize(cols, rows)` | 调整 terminal 和 session |
| `copy_selection()` / `paste(text)` | 剪贴板操作 |
| `zoom_in/out/reset()` | 缩放（范围 0.5-3.0） |
| `update_search()` | 搜索可见文本 |

**输出处理流程**：
`Session` → `broadcast channel` → 后台线程（`start_output_processing`）→ `output_buffer` → `drain_output()` → `Terminal::write()`

**按键编码**：Ctrl+A-Z → 0x01-0x1A，特殊键（Enter、Backspace、Tab、Escape、方向键等）→ 对应转义序列，Shift 修饰键处理，普通字符透传。

## 常见任务

### 添加新的快捷键处理
1. 在 `pureterm-core` 的 `hotkey.rs` 添加快捷键定义
2. 在 `app.rs` 的 `handle_key` 中添加匹配和处理逻辑

### 修改标签页行为
编辑 `tabs/manager.rs`。`open_tab` 和 `close_tab` 管理标签生命周期。

### 修改分割面板逻辑
编辑 `split/container.rs`。注意 `split_pane` 的递归逻辑和 `close_pane` 的空容器清理。

### 修改按键映射
编辑 `terminal/pane.rs` 的 `encode_key` 方法。特殊键通过 match 映射到转义序列。

### 添加新的 UI 组件
1. 在 `src/` 下创建模块目录
2. 在 `lib.rs` 中导出
3. 在 `app.rs` 中集成到应用状态
