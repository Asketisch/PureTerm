# Dual Sidebar（双侧边栏）

## 概述

可隐藏的双侧边栏——左侧显示 Workspace 列表 + Git 分支状态，右侧显示多 mode（files / find / sessions / feed）。宽度可拖拽调整（默认 240px，范围 120-480px）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | 垂直 Tabs（Workspaces）+ 水平 Surface tabs，Workspace Groups 侧边栏 | `Sources/Sidebar/` |
| **iTerm2** | Toolbelt（11 tools：command history, directories, jobs, notes, paste history, profiles, snippets 等） | `sources/Toolbelt/` |
| **Tabby** | 左侧 Profile 列表 | — |

## PureTerm 现状

**未实现。** PureTerm 当前使用命令面板（`Cmd+Shift+P`）进行导航，没有持久化的侧边栏 UI。Status Bar（`crates/pureterm/src/status_bar.rs`）仅显示 24px 底部信息栏。

## 优先级

**P1 — AI 差异化能力**

侧边栏是 workspace 管理和 AI 交互的入口。cmux 的侧边栏是其核心 UI 模式。

## 工作量

**L（1.5-2 周）**

需要：侧边栏容器 widget、可拖拽分割、多 mode 切换、Git 集成。

## 实现方向

1. 新增 `crates/pureterm/src/sidebar/` 模块
2. 左侧栏：`Cmd+B` 切换显示，Workspace 列表 + Git 分支（`git2` crate）
3. 右侧栏：`Cmd+Shift+B` 切换，多 tab mode（Files / Find / Sessions / Feed）
4. 宽度可拖拽：分隔线 drag handler
5. 每个 mode 独立的 iced widget
6. 配置：`sidebar.left_width`, `sidebar.right_width`, `sidebar.left_default_open`
