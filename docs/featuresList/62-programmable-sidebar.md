# 可编程 Sidebar（DSL 解释器 + IPC）

## 概述

允许用户通过 DSL 脚本（Rhai 或 WASM）定义自定义侧边栏内容。Sidebar 文件声明触发器 + 渲染回调，DSL 解释器订阅事件总线，实时聚合数据并渲染。支持沙箱（资源限制 + 权限隔离）。

包含两个子特性：
- **GAP-3.2**：DSL 解释器（Rhai/WASM 沙箱执行）
- **GAP-6.8**：IPC 侧（解释器订阅事件流，sidebar 视图 = 事件流聚合 + 用户渲染回调）

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | Sidebar Interpreter（`Sources/Sidebar/Interpreter/`），支持用户定义 sidebar 内容 | `Sources/Sidebar/Interpreter/` |

## PureTerm 现状

**未实现。** PureTerm 的 Event System（`crates/pureterm/src/feed_log.rs`）已有 12 类事件和 EventBus，但没有 DSL 解释器和可编程 sidebar 框架。

**依赖**：
- GAP-3.1（双侧边栏框架）— 提供 sidebar 容器
- GAP-6.7（事件流）— 提供数据源

## 优先级

**P2 — 终端增强**

可编程 sidebar 是高级用户的需求，需要先实现侧边栏框架和事件流。

## 工作量

**L（1.5-2 周）**

需要：DSL 解释器（`rhai` crate）、沙箱、事件订阅、渲染回调。

## 实现方向

1. DSL 引擎：`rhai` crate（轻量、沙箱友好）
2. Sidebar 文件格式：`~/.config/pureterm/sidebars/*.rhai`
3. 示例：`show-git.rhai` — 订阅 `session.*` 事件，实时显示 git 状态
4. 沙箱：`rhai::Engine::new()` + 资源限制（最大执行时间、内存）
5. 渲染回调：DSL 返回 `Vec<SidebarItem>`（图标 + 文本 + 动作）
6. 事件订阅：解释器通过 IPC 连接 Event Bus，订阅指定事件类型
7. DoD：写一个 `show-git.rhai`，实时显示当前 git 分支和 dirty 状态
