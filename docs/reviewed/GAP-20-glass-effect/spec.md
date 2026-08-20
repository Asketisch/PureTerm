---
gap: GAP-20
title: 玻璃效果
category: UI/外壳
plan_type: platform
priority: P4
effort: M
status: done
closed: 2026-06-29
---

# Glass Effect（macOS 玻璃效果）

## 概述

macOS 26+ 使用 `NSGlassEffectView` 实现系统级毛玻璃效果；macOS 13-15 回退 `NSVisualEffectView`。窗口 `setOpaque(false)` + 主背景 alpha=0 实现透明。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | GlassEffectView（`Sources/GlassEffectView.swift`） | `Sources/GlassEffectView.swift` |
| **参考实现 A** | `window_background_opacity` + `macos_window_background_blur`（业界终端原生支持） | — |
| **参考实现 B** | 透明度/模糊（Windows/macOS，通过 Electron） | — |

## PureTerm 现状

**未实现。** PureTerm 的 Iced 窗口默认不透明，无透明度/模糊配置。

## 优先级

**P4 — 视觉/分发**

Glass Effect 是视觉美化功能，对功能无影响。macOS 26 的 NSGlassEffectView 是新 API，需要等待系统普及。

## 工作量

**M（1 周）**

macOS：`objc2` 调用 `NSGlassEffectView` / `NSVisualEffectView`。需要修改 Iced 窗口创建逻辑。

## 实现方向

1. macOS：通过 `objc2` 获取 `NSWindow`，设置 `opaque = false`
2. macOS 26+：创建 `NSGlassEffectView` 作为 content view
3. macOS 13-25：创建 `NSVisualEffectView`（`.behindWindow` blending mode）
4. 配置：`window.glass_effect = true`, `window.background_opacity = 0.8`
5. 跨平台：Linux/Windows 无此效果（回退到普通透明度）
