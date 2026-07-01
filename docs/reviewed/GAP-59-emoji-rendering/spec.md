---
gap: GAP-59
title: Emoji Rendering
category: 视觉
plan_type: enhance
priority: P4
effort: M
status: done
closed: 2026-07-02
reviewed: 2026-07-01
---

# Emoji Rendering（Emoji 渲染）

## 概述

正确渲染 Emoji 字符（包括变体选择符、ZWJ 序列、肤色修饰符），支持 Emoji 专用着色器通道。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Kaku** | 专用 Emoji 着色器通道 | `Kaku-main/crates/termwiz/src/shader.wgsl` |
| **iTerm2** | Emoji 渲染支持 | — |

## PureTerm 现状

**Done（2026-07-01，config + container）。** `emoji_rendering.rs` 扩展：`EmojiRenderingConfig` 增加 `font_fallback_secondary`（默认 `Noto Color Emoji`），`fallback_chain()` 按优先级产出字体名（自动跳过空字符串）。`EmojiGlyph { character, width, height, data }` 作为 emoji 光栅化结果容器，RGBA8 premultiplied 字节序（与 crossfont::BitmapBuffer::Rgba 一致），`has_visible_content()` 扫 alpha 通道判断是否全透明。`is_complex_emoji` 维持 ZWJ / 多字符判定。专用 GPU atlas 通道的纹理上传和着色器按后续任务接入。6 单测覆盖 ZWJ detect / fallback chain 顺序 / 跳过空 / dedicated_channel 双开关 / 容器可见性。

## 优先级

**P4 — 视觉/分发**

Emoji 渲染是基础能力，当前实现已覆盖大部分场景。

## 工作量

**M（1 周）**

需要：Emoji 字体回退、专用纹理 atlas。

## 实现方向

1. Emoji 字体：`Noto Color Emoji` 或 `Apple Color Emoji`
2. 字体回退：文本 → Emoji 字体 → CJK 字体
3. 纹理 atlas：Emoji 专用 texture page（类 Kaku）
4. ZWJ 序列：确保 Unicode 14+ 支持
5. 性能：Emoji atlas 缓存，避免重复光栅化
