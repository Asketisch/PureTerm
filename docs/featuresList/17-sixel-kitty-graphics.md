# Sixel & Kitty Graphics（终端内图片渲染）

## 概述

在终端内渲染图片——Sixel（DEC 标准，广泛支持）和 Kitty Graphics Protocol（现代终端常用）。用于显示截图、图表、AI 生成的图片等。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | Sixel Graphics（DCS hook-based）+ Kitty Graphics Protocol（完整支持） | `sources/VT100/VT100SixelParser.h/m`, `sources/InlineImages/` (KittyImageCommand.swift, KittyImageController.swift) |
| **Kaku** | 通过 WezTerm 内核支持 Sixel + Kitty | — |

## PureTerm 现状

**未实现。** PureTerm 的终端渲染（`crates/pureterm/src/terminal_shader.rs`）仅支持文本和字形，不支持图片协议。

## 优先级

**P3 — 兼容性/国际化**

图片渲染在 AI 可视化场景下有用（如显示 AI 生成的图表），但大多数终端用户不常用。

## 工作量

**L（1.5-2 周）**

需要：Sixel/Kitty 协议解析、图片解码、GPU 纹理渲染。

## 实现方向

1. Sixel 解析：DCS hook 解码 Sixel 数据 → RGB bitmap
2. Kitty 解析：APC 命令解码 → RGB bitmap
3. GPU 渲染：将 bitmap 上传为纹理，在 `terminal_shader.rs` 中添加图片渲染通道
4. 图片缓存：LRU cache 避免重复解码
5. 暴露给 MCP：`capture_screenshot()` 返回终端内图片
