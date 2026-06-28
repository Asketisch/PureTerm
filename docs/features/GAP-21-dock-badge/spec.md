---
gap: GAP-21
title: Dock Badge
category: UI/外壳
plan_type: wrap-up
priority: P4
effort: S
status: in-progress
closed: null
---

# Dock Badge（macOS Dock 角标）

## 概述

在 macOS Dock 图标上显示角标数字，反映需要用户关注的事项数量（Feed 待审批数、通知数等）。Dock 右键菜单显示最近 Workspace。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | Dock 徽章 + Dock 右键菜单显示最近 Workspace | `Sources/DockController.swift` |
| **iTerm2** | macOS 通知角标 | — |

## PureTerm 现状

**部分实现（通知未读角标 ✅；Feed / Workspace 菜单 ❌）。** 通知未读数 Dock 角标作为 [GAP-09 Notification System](../GAP-09-notification/spec.md) 子能力交付，非独立模块。

**已实现（2026-06-28，GAP-09）：**
- `macos_dock_badge.rs` — `NSDockTile.setBadgeLabel` / `clear_badge`
- `iced_app.rs` `refresh_dock_badge()` — 在 notification post、bell、pane 聚焦、`DismissAllNotifications` 后刷新
- 配置门控：`[notification].dock_badge`（默认 true）

**未实现：**
- Feed 待审批数叠加（GAP-7.4 Feed 面板计数）
- Dock 右键最近 Workspace 菜单

## 优先级

**P4 — 视觉/分发**

Dock Badge 是轻量级的通知提示，实现简单但优先级低。通知角标已由 GAP-09 覆盖。

## 工作量

**S（2-3 天）**

macOS：`NSApplication.dockTile().setBadgeLabel()` + `objc2` 调用。通知角标部分已完成。

## 实现方向

1. ✅ macOS：通过 `objc2` 调用 `NSDockTile.setBadgeLabel()`（通知未读数）
2. 🚧 角标数字来源：通知未读 ✅；Feed 待审批 ❌
3. ✅ 订阅通知 store 的 unread 变化（非 EventBus 间接路径）
4. ❌ Dock 右键菜单：通过 `NSMenu` 添加最近 Workspace 列表
5. ✅ 配置：`[notification].dock_badge = true`
