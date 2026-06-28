---
gap: GAP-55
title: Transparency & Blur
category: 视觉
plan_type: feature
priority: P4
effort: M
status: not-started
closed: null
---

# Transparency & Blur（透明度和模糊）

## 概述

为终端窗口设置背景透明度和高斯模糊效果，让用户可以看到终端背后的内容。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | 透明度/模糊（Windows/macOS，通过 Electron） | — |
| **Kaku** | `window_background_opacity` + `macos_window_background_blur`（WezTerm 原生） | — |

## PureTerm 现状

**未实现。**

## 优先级

**P4 — 视觉/分发**

透明度/模糊是视觉功能，对工作效率无影响。

## 工作量

**M（1 周）**

macOS：`NSVisualEffectView` 模糊。跨平台：GPU shader 模糊。

## 实现方向

1. 配置：`window.opacity = 0.9`, `window.blur = true`
2. macOS：`NSVisualEffectView`（`.behindWindow` blending mode）
3. Linux：Compiz/KWin blur hint
4. Windows：`DwmEnableBlurBehindWindow`
5. GPU 模糊：在 `terminal_shader.rs` 中添加 Gaussian blur pass
