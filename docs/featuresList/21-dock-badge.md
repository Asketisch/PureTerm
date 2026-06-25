# Dock Badge（macOS Dock 角标）

## 概述

在 macOS Dock 图标上显示角标数字，反映需要用户关注的事项数量（Feed 待审批数、通知数等）。Dock 右键菜单显示最近 Workspace。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | Dock 徽章 + Dock 右键菜单显示最近 Workspace | `Sources/DockController.swift` |
| **iTerm2** | macOS 通知角标 | — |

## PureTerm 现状

**未实现。** PureTerm 的 Event System 记录事件，但没有与 macOS Dock 交互。

## 优先级

**P4 — 视觉/分发**

Dock Badge 是轻量级的通知提示，实现简单但优先级低。

## 工作量

**S（2-3 天）**

macOS：`NSApplication.dockTile().setBadgeLabel()` + `objc2` 调用。

## 实现方向

1. macOS：通过 `objc2` 调用 `NSDockTile.setBadgeLabel()`
2. 角标数字来源：Feed Approval 待审批数 + 通知数
3. 订阅 Event System 的 `FeedRequested` / `Notification` 事件
4. Dock 右键菜单：通过 `NSMenu` 添加最近 Workspace 列表
5. 配置：`dock.badge_enabled = true`
