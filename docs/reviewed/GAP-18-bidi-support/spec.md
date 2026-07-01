---
gap: GAP-18
title: BiDi Support
category: 终端交互
plan_type: feature
priority: P3
effort: L
status: done
closed: 2026-06-29
---

# BiDi 双向文本支持

## 概述

支持阿拉伯文、希伯来文等 RTL（从右到左）文本的正确渲染。需要 Unicode BiDi 算法重排序与字体整形（HarfBuzz）联动。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | Full bidirectional text support with RTL annotation | `sources/Drawing/BidiDisplayInfo.swift` |
| **Ghostty** | 完整 Unicode BiDi 支持 | `src/unicode/` |

## PureTerm 现状

**未实现（P3）。** PureTerm 的文本渲染按 LTR 方向处理，不支持 BiDi 重排序。

## 优先级

**P3 — 兼容性/国际化**

BiDi 是国际化的基础需求，但 PureTerm 的主要用户群（开发者）大多使用 LTR 文本。

## 工作量

**L（1.5-2 周）**

需要：`unicode-bidi` crate 集成、HarfBuzz 整形、渲染管线修改。

## 实现方向

1. 集成 `unicode-bidi` crate 做 BiDi 重排序
2. 在 Grid 层面存储每个字符的 BiDi level
3. 渲染时根据 level 决定视觉顺序
4. 与 HarfBuzz 整形联动（连字、mark positioning）
5. 配置选项：`bidi_mode = "auto" | "ltr" | "rtl"`
