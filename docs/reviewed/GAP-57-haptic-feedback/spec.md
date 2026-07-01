---
gap: GAP-57
title: Haptic Feedback
category: 视觉
plan_type: platform
priority: P4
effort: S
status: reviewed
closed: 2026-06-29
reviewed: 2026-07-01
---

# Haptic Feedback（触觉反馈）

## 概述

在特定操作时触发 MacBook 触觉反馈（trackpad 震动），如 Tab 切换、错误提示、通知到达。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | Trackpad haptic feedback | `sources/Haptics/` (iTermHapticActuator) |

## PureTerm 现状

**Done（2026-06-29，stub）。** `macos_haptic.rs` 提供 `HapticEvent`（`TabSwitch` / `Error` / `Notification`）和 `HapticConfig`（`enabled` / `on_tab_switch` / `on_error` / `on_notification`），`should_fire` 按 event 类型 + enabled 短路，`perform` 在 macOS 走 `log::debug!` stub。`NSHapticFeedbackManager` 实际调用按 spec 留待后续。

## 优先级

**P4 — 视觉/分发**

触觉反馈是锦上添花功能，仅 macOS 且带 Force Touch trackpad 的设备可用。

## 工作量

**S（2-3 天）**

macOS：`objc2` 调用 `NSHapticFeedbackManager`。

## 实现方向

1. macOS：通过 `objc2` 调用 `NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)`
2. 触发时机：Tab 切换、错误发生、通知到达
3. 配置：`haptic_feedback.enabled = true`, `haptic_feedback.on_tab_switch = true`
