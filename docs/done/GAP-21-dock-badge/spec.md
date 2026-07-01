---
gap: GAP-21
title: Dock Badge
category: UI/外壳
plan_type: wrap-up
priority: P4
effort: S
status: done
closed: 2026-06-28
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

**已完成（2026-06-28）。** 通知未读角标作为 [GAP-09 Notification System](../GAP-09-notification/spec.md) 子能力先行交付；本 wrap-up 补齐 Feed 叠加与 Dock 菜单。

**已实现：**
- `macos_dock_badge.rs` — `NSDockTile.setBadgeLabel` / `clear_badge`
- `iced_app.rs` `refresh_dock_badge()` / `dock_badge_total()` — 通知 unread + `feed_panel.pending_count()` 求和；在 notification post、bell、pane 聚焦、`DismissAllNotifications`、Feed resolve/timeout/inbound 后刷新
- `macos_dock_menu.rs` — `applicationDockMenu:` + `puretermDockWorkspace:`；MRU 最近 10 个 workspace（`recent_workspace_ids`）；`TerminalWakeup` tick 同步菜单并消费选择 → `ActivateWorkspaceById`
- 配置门控：`[notification].dock_badge`（默认 true）

## 优先级

**P4 — 视觉/分发**

## 工作量

**S（2-3 天）**

## 实现方向

1. ✅ macOS：通过 `objc2` 调用 `NSDockTile.setBadgeLabel()`
2. ✅ 角标数字来源：通知 unread + Feed 待审批
3. ✅ 订阅通知 store 的 unread 变化（非 EventBus 间接路径）
4. ✅ Dock 右键菜单：通过 `NSMenu` 添加最近 Workspace 列表
5. ✅ 配置：`[notification].dock_badge = true`

## 单测

- `iced_app::dock_badge_total` — unread + Feed 求和
- `workspace_e2e::touch_recent_workspace_mru_order` — MRU 顺序与上限
