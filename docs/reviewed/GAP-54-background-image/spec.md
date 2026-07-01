---
gap: GAP-54
title: Background Image
category: 视觉
plan_type: feature
priority: P4
effort: M
status: reviewed
closed: 2026-06-29
reviewed: 2026-07-01
---

# Background Image（背景图片）

## 概述

为终端窗口设置自定义背景图片，支持透明度调整、图片位置/缩放配置。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | 自定义背景图片（插件支持） | — |
| **Kaku** | `window_background_opacity` + `macos_window_background_blur` | — |

## PureTerm 现状

**Done（2026-06-29，配置 scaffold）。** `background_image.rs` 提供 `BackgroundMode`（`Center` / `Tile` / `Stretch`）和 `BackgroundImageConfig`（`enabled` / `image` / `opacity` 默认 `0.35` / `mode`），`resolved_path` 仅在 `enabled` 且路径存在时返回 `Some`。GPU 纹理通道按 spec 留待后续。

## 优先级

**P4 — 视觉/分发**

背景图片是纯视觉功能，优先级最低。

## 工作量

**M（1 周）**

需要：图片解码、GPU 纹理渲染、配置。

## 实现方向

1. 配置：`background.image = "/path/to/image.png"`, `background.opacity = 0.5`
2. 渲染：在 `terminal_shader.rs` 中添加背景纹理通道
3. 图片加载：`image` crate 解码 → GPU 纹理
4. 位置：`center`, `tile`, `stretch`
5. 与 Glass Effect（20）互斥
