---
gap: GAP-19
title: Dual Sidebar
category: UI/外壳
plan_type: feature
priority: P1
effort: L
status: done
closed: 2026-06-29
review: 2026-07-01
---

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

**MVP 已实现（2026-06-29）；Review ✅（2026-07-01）。** 双侧边栏：`crates/pureterm/src/sidebar/`；主布局 `left | terminal | right`（`iced_app.rs`）。⌘B 切换左侧（Workspace 列表 + Git 分支摘要），⌘⇧B 切换右侧（Find / Feed / Git 三 tab）。Git 面板与 Feed 审批面板已从 overlay 迁入右侧栏；Find 复用 ⌘F/⌘⇧F 搜索状态。配置：`[sidebar] left_width` / `right_width`（默认 240px，120–480 clamp）。

**Review 收尾（2026-07-01）三件套：**

1. **每窗口独立状态** — `SidebarState`（含 `left_width`/`right_width` 运行时宽度）从 `PureTermApp` 全局字段迁到 `Window` 上；每窗独立 开/关/tab/宽度，A 窗 toggle 不再影响 B 窗。视图按 `view(id)` 的 `id` 取窗（顺带修了「B 窗渲染 A 窗 workspace 列表」的 latent bug）。新窗按 `[sidebar] left_default_open` 起始。
2. **拖拽调宽** — 左/右分隔线 5px `mouse_area` 命中区，按下发 `SidebarDividerPress(Side)`；实时拖动复用既有 `GlobalCursorMoved`/`GlobalMouseRelease` 机（同 pane divider），按 `pos.x`（左）或 `viewport_w - pos.x`（右）算新宽并 clamp 120–480，逐帧 `resize_active_tab_panes`。宽度仅运行时，重启回配置默认（持久化见 follow-up）。
3. **事件对称性修 2 bug** — `ToggleRightSidebar` 之前在「关→开且当前 Git tab」早 return，漏发 `sidebar.right_opened`（BUG-1）；关闭分支调 `git_panel.close()` 不发 `git.panel_closed`，与 `open_git_sidebar_tab` 的 `git.panel_opened` 失配（BUG-2）。抽纯函数 `sidebar::right_sidebar_event(now_open)` 锁「每次 toggle 恒发」契约。

## 优先级

**P1 — AI 差异化能力**

侧边栏是 workspace 管理和 AI 交互的入口。cmux 的侧边栏是其核心 UI 模式。

## 工作量

**L（1.5-2 周）**

需要：侧边栏容器 widget、可拖拽分割、多 mode 切换、Git 集成。

## 实现方向

1. 新增 `crates/pureterm/src/sidebar/` 模块 ✅
2. 左侧栏：`Cmd+B` 切换显示，Workspace 列表 + Git 分支（`git2` crate）✅
3. 右侧栏：`Cmd+Shift+B` 切换，多 tab mode（Files / Find / Sessions / Feed）✅
4. 宽度可拖拽：分隔线 drag handler ✅（Review 2026-07-01 落地，复用 `GlobalCursorMoved` 机）
5. 每个 mode 独立的 iced widget ✅
6. 配置：`sidebar.left_width`, `sidebar.right_width`, `sidebar.left_default_open` ✅

## Follow-up（未在本次 review 修）

- **拖拽宽度不持久化**：仅运行时，重启回 `[sidebar]` 配置默认（持久化需走 `mutate_config` scoped-write，或写 workspaces.json）。
- **`viewport_size` 全局**：多窗各自尺寸仍用单一全局字段（既有局限，`WindowResized` 只更一个），sidebar 拖拽沿用之；per-window viewport 属 GAP-25 范畴。
- **拖拽逐帧 PTY resize**：同 pane divider 既有模式，可能略顿；防抖为后续优化。
