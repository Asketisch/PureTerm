# Workspace Groups（工作区分组）

## 概述

将 Workspace 按项目/任务/客户分组，在侧边栏显示分组结构。支持分组展开/折叠、分组级别操作（一次性打开分组内所有 workspace）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | Workspace Groups 侧边栏分组 | `Sources/Sidebar/` |

## PureTerm 现状

**未实现。** PureTerm 有 Workspace 概念（`crates/pureterm/src/iced_app.rs`），但没有分组功能。

## 优先级

**P3 — 兼容性/国际化**

Workspace Groups 在管理大量 workspace 时有用，但非核心功能。

## 工作量

**S（3-5 天）**

核心是配置格式扩展 + 侧边栏分组显示。

## 实现方向

1. 配置：`[[workspace_groups]]` 段落，每个 group 包含 workspace 列表
2. 侧边栏显示：树形结构，group → workspace
3. 分组操作：展开/折叠、打开全部、关闭全部
4. 拖拽：workspace 在 group 间拖拽
5. 持久化：`gui-sessions.json` 中存储 group 结构
