---
gap: GAP-28
title: Window Border
category: UI/外壳
plan_type: feature
priority: P4
effort: S
status: done
closed: 2026-06-29
---

# Window Border（窗口边框）

## 概述

为终端窗口添加自定义边框样式，支持颜色主题感知、活动/非活动窗口不同边框样式。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | `WindowBorderView`（自定义窗口边框） | `sources/TerminalView/` |

## PureTerm 现状

**未实现。** PureTerm 使用操作系统默认窗口边框。

## 优先级

**P4 — 视觉/分发**

Window Border 是纯视觉功能，优先级最低。

## 工作量

**S（2-3 天）**

在 Iced 窗口外层添加自定义边框 widget。

## 实现方向

1. Iced widget：在窗口内容外层包裹 `Border` widget
2. 配置：`window.border.color`, `window.border.width`
3. 活动/非活动：根据窗口焦点状态切换样式
4. 主题感知：跟随 terminal color scheme
