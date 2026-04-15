# 13 - 搜索系统

## 功能概述

终端内容搜索：支持正则表达式搜索、后台搜索线程、跨页面（回滚缓冲区）搜索、视口搜索、搜索高亮。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `terminal/search/active.zig` | ~300 | 活动区域搜索 |
| `terminal/search/pagelist.zig` | ~300 | PageList 搜索 |
| `terminal/search/screen.zig` | ~400 | Screen 级搜索（组合） |
| `terminal/search/sliding_window.zig` | ~500 | 滑动窗口搜索缓冲区 |
| `terminal/search/Thread.zig` | ~400 | 后台搜索线程 |
| `terminal/search/viewport.zig` | ~300 | 视口搜索 |
| `terminal/StringMap.zig` | ~400 | 字符串到屏幕位置映射 |

## 核心数据结构

### 1. 滑动窗口 (`sliding_window.zig`)

跨页面搜索的核心数据结构：

```
SlidingWindow:
  ├── buffer: CircBuf(PageEntry)   - 循环缓冲区
  ├── text: ArrayList(u8)          - 编码后的文本
  ├── direction: Direction         - forward/backward
  ├── overlap: usize               - 跨页重叠字节数
  └── 方法:
        ├── addPage()              - 添加页面文本
        ├── search()               - 在窗口内搜索
        └── advance()              - 向前/向后移动窗口
```

### 2. 搜索类型

```
ActiveSearch - 在可变的活动区域搜索
  └── 处理活动区域随新行添加而移动的情况

PageListSearch - 在不可变的历史页面搜索
  └── 使用 tracked pin 和滑动窗口

ScreenSearch - 组合搜索（活动 + 历史）
  ├── active: ActiveSearch
  ├── pagelist: PageListSearch
  ├── cached_results
  └── 处理 resize、scrollback pruning 等状态变化

ViewportSearch - 仅搜索可见视口
  ├── fingerprinting 变更检测
  └── dirty tracking 避免重复搜索
```

### 3. 后台搜索线程 (`Thread.zig`)

```
SearchThread:
  ├── event_loop: xev             - 独立事件循环
  ├── mailbox                     - 消息队列
  ├── timer                       - 定时刷新
  └── 方法:
        ├── threadMain()          - 线程入口
        ├── search()              - 执行搜索
        └── reportResults()       - 报告结果
```

### 4. StringMap (`StringMap.zig`)

```
StringMap:
  ├── string: []const u8          - 编码后的文本
  ├── mapping: []const Mapping    - 字节 → 屏幕 pin 映射
  └── 方法:
        ├── searchIterator(regex) - 正则搜索迭代器
        └── 返回: 匹配 + 屏幕位置
```

## 纯 Rust 实现要点

1. **正则用 `regex` crate**
2. **滑动窗口用 `VecDeque`**
3. **搜索必须用后台线程**：避免阻塞主线程，保证终端响应
4. **高亮通过渲染系统**：将匹配位置传递给渲染器
5. **StringMap 用 `(byte_offset, grid_position)` 映射**

```rust
pub struct SearchState {
    query: Option<regex::Regex>,
    matches: Vec<SearchMatch>,
    selected: Option<usize>,
}

pub struct SearchMatch {
    start: GridPosition,
    end: GridPosition,
}
```
