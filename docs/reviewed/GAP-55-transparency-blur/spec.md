---
gap: GAP-55
title: Transparency & Blur
category: 视觉
plan_type: feature
priority: P4
effort: M
status: done
closed: 2026-07-02
reviewed: 2026-07-01
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

**Done（2026-07-01，macOS）。** `window_effects.rs` 扩展 `WindowEffectsConfig`（新增 `blending_mode` = `within_window` | `behind_window`）和 `effective_opacity`（base × opacity，clamp `[0.05, 1.0]`），新增 `BlurApplyOutcome` 和 `apply_blur_macos` 走 `objc2 0.6 + objc2-app-kit 0.3.2`：取 `NSApplication::windows()`，对每个 `NSWindow` 分配一个 `NSVisualEffectView`（UnderWindowBackground + FollowsWindowActiveState + NSViewWidthSizable|NSViewHeightSizable），把原 content view 作为子视图塞进 effect view，最后 `setContentView`。Windows 路径返回 `UnsupportedPlatform`（DWM blur-behind 需后续 winit HWND 集成）。6 单测覆盖 disabled / 关闭 blur / 越界 opacity / TOML 缺失路径 / mode 解析 / passthrough。

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
