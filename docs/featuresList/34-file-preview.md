# File Preview（文件预览）

## 概述

在终端内预览文件内容：图片（缩略图）、PDF（第一页）、文本（语法高亮）、QuickLook（macOS）。支持从终端输出中检测文件路径并预览。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | 图片/PDF（缩略图）/Text/QuickLook/媒体播放 | — |
| **iTerm2** | QuickLook for selected text + Portholes（JSON/Markdown/Text in-terminal viewers） | `sources/QuickLook/` (iTermQuickLookController), `sources/Portholes/` (JSONPortholeRenderer, MarkdownPortholeRenderer, TextViewPorthole) |

## PureTerm 现状

**未实现。** PureTerm 的内嵌浏览器（`crates/pureterm/src/browser/`）可显示网页，但不是通用文件预览。

## 优先级

**P3 — 兼容性/国际化**

文件预览可通过外部工具（如 `bat`, `preview`）替代。

## 工作量

**M（1 周）**

需要：文件类型检测、图片解码、PDF 渲染、语法高亮。

## 实现方向

1. 智能选择增强：检测文件路径后提供"预览"选项
2. 图片：`image` crate 解码 → 显示缩略图
3. PDF：`pdf` crate 或调用系统 `mdls`
4. 文本：`syntect` 语法高亮 → 内嵌预览 pane
5. macOS QuickLook：`objc2` 调用 `QLPreview`
6. 快捷键：`Cmd+Y` 预览选中文件
