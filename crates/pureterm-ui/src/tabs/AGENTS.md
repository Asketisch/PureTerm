# pureterm-ui/src/tabs — 标签页管理

标签页生命周期管理：创建、关闭、选择、恢复。标签内容可以是单个 TerminalPane 或 SplitContainer（分割布局）。

## 文件结构

```
src/
├── mod.rs     # 模块重导出 (3 行)
└── manager.rs # TabManager + Tab + TabContent + ClosedTab (223 行)
```

## 核心类型

### Tab（manager.rs）

```rust
pub struct Tab {
    pub id: u64,
    pub title: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub content: TabContent,
    pub has_activity: bool,  // 有未读活动
    pub has_focus: bool,
    pub created_at: Instant,
}
```

### TabContent（manager.rs）

```rust
pub enum TabContent {
    Pane(u64),        // 单个终端窗格
    Split(SplitRef),  // 分割容器
}

pub struct SplitRef {
    pub root_id: u64,  // SplitContainer 的根容器 ID
}
```

### ClosedTab（manager.rs）

已关闭标签的恢复信息：

```rust
pub struct ClosedTab {
    pub id: u64,
    pub title: String,
    pub closed_at: Instant,
}
```

### TabManager（manager.rs）

```rust
pub struct TabManager {
    tabs: Vec<Tab>,
    active_tab: Option<u64>,
    closed_tabs_stack: Vec<ClosedTab>,  // 最近关闭的标签栈（用于恢复）
    next_id: u64,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new()` | 创建空管理器 |
| `open_tab(content, title)` | 打开新标签，返回标签 ID，自动设为激活 |
| `close_tab(id)` | 关闭标签，返回 ClosedTab（入栈供恢复），如有则自动切换到另一标签 |
| `select_tab(id)` | 选中指定标签 |
| `previous_tab()` / `next_tab()` | 切换到上一个/下一个标签 |
| `restore_last_closed()` | 恢复最近关闭的标签（从 closed_tabs_stack 弹出） |
| `set_title(id, title)` | 设置标签标题 |
| `mark_activity(id)` | 标记标签有未读活动 |
| `tab_ids()` | 获取所有标签 ID |
| `find_tab(id)` | 查找指定 ID 的标签 |

## 父级导航

← [pureterm-ui/AGENTS.md](../../AGENTS.md)
