---
gap: GAP-54
title: Background Image
category: 视觉
plan_type: feature
priority: P4
effort: M
status: done
closed: 2026-07-02
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

**Done（2026-07-01）。** `background_image.rs` 扩展：`BackgroundMode` + `BackgroundImageConfig`（保留 `resolved_path`），新增 `DecodedBackground`（RGBA8 + w/h）+ `decode_rgba8` / `decode_rgba8_from_bytes`（走 `image` 0.25，仅 PNG/JPEG，default-features 关掉 webp/heif）+ `load_decoded`。`terminal_shader.rs` 新增 `BgImageVertex` + `ScreenUniforms` 增加 `bg_opacity` / `bg_mode` / `bg_img_width` / `bg_img_height`，新增 `bg_image_pipeline` + `bg_image_bgl` + `bg_image_sampler`（Linear + Repeat），`prepare()` 按 `mode` 在 CPU 端组子矩形（Center 居中按图像 native 尺寸，Stretch/Tile 全屏），`draw()` 先画背景再画 bg_color/text。`shaders/bg_image.wgsl` 做纹理采样 + opacity 乘法 + 圆角裁剪。`iced_app.rs` 启动时按 `p4.background_cfg` 解码一次并把 `DecodedBackground` + 签名（FNV-1a of RGBA bytes）共享给每个 `TerminalProgram`，GPU 侧只在 signature 变化时重传纹理。4 单测覆盖 disabled / 无效路径 / 无效字节 / mode 解析。

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
