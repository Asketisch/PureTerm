---
gap: GAP-4.6
title: Accessibility
category: 视觉
plan_type: platform
priority: P4
effort: M
status: done
closed: 2026-06-29
---

# Accessibility（VoiceOver + 无障碍）

## 概述

确保所有 UI 元素对屏幕阅读器（VoiceOver/NVDA）可读，包括：Tab 切换、命令面板弹出、状态栏更新、分屏操作等都有语音提示。

包含原 GAP-4.6 的两个子项：
- **VoiceOver**：所有 iced widget 通过 `accessible()` 属性提供文本描述
- **Touch Bar**：已合并到 `../GAP-22-touch-bar/spec.md`，此处仅保留 VoiceOver

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Ghostty** | 完全 VoiceOver 支持 | `Surface.zig` |
| **iTerm2** | Accessibility 支持 | `sources/Accessibility/` |

## PureTerm 现状

**未实现。** Iced 0.14 支持 `widget::accessible()` API，但 PureTerm 未使用。

## 优先级

**P4 — 视觉/分发**

Accessibility 是合规需求（WCAG），但对大部分用户无直接影响。

## 工作量

**M（1 周）**

需要：为所有 iced widget 添加 `accessible()` 属性。

## 实现方向

1. 为每个 widget 添加 `accessible()` 描述：
   - Tab：`"Tab 1: bash - ~/Projects"`
   - Command Palette：`"Command Palette, 5 个结果"`
   - Status Bar：`"Status Bar: main branch, SSH connected"`
2. 操作反馈：Tab 切换 → `"Switched to Tab 2"`
3. 分屏反馈：`"Split vertically, 2 panes"`
4. 配置：`accessibility.enabled = true`
