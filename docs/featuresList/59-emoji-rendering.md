# Emoji Rendering（Emoji 渲染）

## 概述

正确渲染 Emoji 字符（包括变体选择符、ZWJ 序列、肤色修饰符），支持 Emoji 专用着色器通道。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Kaku** | 专用 Emoji 着色器通道 | `Kaku-main/crates/termwiz/src/shader.wgsl` |
| **iTerm2** | Emoji 渲染支持 | — |

## PureTerm 现状

**基础实现。** PureTerm 通过 crossfont + Alacritty 内核支持基本 Emoji 渲染，但可能不支持复杂 ZWJ 序列。

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
