---
gap: GAP-56
title: Focus Follows Mouse
category: 视觉
plan_type: feature
priority: P4
effort: S
status: reviewed
closed: 2026-06-29
reviewed: 2026-07-01
---

# Focus Follows Mouse（焦点跟随鼠标）

## 概述

X11 风格的焦点行为——鼠标移动到哪个 pane，哪个 pane 就自动获得焦点，无需点击。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | X11-style with CPS integration | `sources/FocusFollowsMouse/` |

## PureTerm 现状

**Done（2026-06-29）。** `focus_follows_mouse.rs` 提供 `FocusFollowsMouseConfig`（`enabled` / `delay_ms` 默认 `150`）和 `FocusFollowsMouseState`（`hover_pane` + `hover_since`），`on_pane_enter` 记录 hover，`pane_to_focus` 在 enabled 且经过 `delay_ms` 后返回 `Some(pane_id)`。Iced `MouseMoved` 事件挂钩按 spec 留待 UI 集成。

## 优先级

**P4 — 视觉/分发**

Focus Follows Mouse 是 Linux/X11 用户的习惯，macOS/Windows 用户不太使用。

## 工作量

**S（2-3 天）**

核心是鼠标移动事件监听 + 焦点切换。

## 实现方向

1. 配置：`focus_follows_mouse = true`
2. 鼠标 hover 检测：iced `on_event` 监听 `MouseMoved`
3. 延迟：可配置（默认 100ms，避免误触）
4. 与 Iced 焦点管理集成
5. 仅在分屏模式下生效
