---
gap: GAP-57
title: Haptic Feedback
category: 视觉
plan_type: platform
priority: P4
effort: S
status: done
closed: 2026-07-02
reviewed: 2026-07-01
---

# Haptic Feedback（触觉反馈）

## 概述

在特定操作时触发 MacBook 触觉反馈（trackpad 震动），如 Tab 切换、错误提示、通知到达。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | Trackpad haptic feedback | `sources/Haptics/` (HapticActuator) |

## PureTerm 现状

**Done（2026-07-01，macOS）。** `macos_haptic.rs` 真实调用 `NSHapticFeedbackManager::defaultPerformer().performFeedbackPattern(_:performanceTime:)`（pattern: `Generic`，time: `Default`），整个 `perform` 体包在 `std::panic::catch_unwind` 边界内（misbehaving performer 不会把 UI 线程拖崩）。新增 `HapticOutcome`（`Skipped` / `Fired` / `Failed(String)`）让调用方 / 测试能区分。`perform_logged` 便利方法吃掉结果并按 debug 级别 log。4 单测覆盖 disabled / 按 kind 启停 / perform 短路 / logged 安全性。

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
