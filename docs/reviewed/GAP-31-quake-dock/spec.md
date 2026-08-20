---
gap: GAP-31
title: Quake 式坞
category: UI/外壳
plan_type: feature
priority: P3
effort: M
status: done
closed: 2026-06-29
---

# Quake-style Dock（下拉式终端）

## 概述

通过全局快捷键从屏幕顶部滑入/滑出终端窗口（类似 Quake 控制台）。支持多显示器、动画过渡。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | 全局快捷键召唤终端（Quake 风格停靠） | — |

## PureTerm 现状

**未实现。** PureTerm 的 Global Hotkeys（`global_hotkeys.rs`）支持全局快捷键，但没有 Quake-style 下拉窗口。

## 优先级

**P3 — 兼容性/国际化**

Quake-style Dock 在 Linux 用户中很流行（如 guake、yakuake），但 macOS 用户不太常用。

## 工作量

**M（1 周）**

需要：全局快捷键监听、窗口位置/动画控制、多显示器支持。

## 实现方向

1. 全局快捷键：`Ctrl+`` 或 `Cmd+``（可配置）
2. 窗口行为：从屏幕顶部滑入，再次按键滑出
3. 动画：iced window position 动画（y 坐标从 -height → 0）
4. 多显示器：在当前显示器顶部显示
5. 配置：`quake.enabled = true`, `quake.hotkey = "Ctrl+Backquote"`, `quake.screen = "current"`
