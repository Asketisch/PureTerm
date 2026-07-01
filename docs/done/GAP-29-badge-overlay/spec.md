---
gap: GAP-29
title: Badge Overlay
category: UI/外壳
plan_type: feature
priority: P4
effort: S
status: done
closed: 2026-06-29
---

# Badge Overlay（徽章覆盖）

## 概述

在终端窗口或 pane 上显示可配置的徽章文字/图标，用于标识环境（如 "PROD"、"STAGING"、"DEV"）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | 可配置 badge overlay（位置、颜色、文字、透明度） | `sources/Drawing/iTermBadgeLabel.h/m` |

## PureTerm 现状

**未实现。**

## 优先级

**P4 — 视觉/分发**

Badge 是运维/DevOps 场景的实用功能，但使用频率不高。

## 工作量

**S（2-3 天）**

在终端渲染层叠加半透明文字。

## 实现方向

1. 配置：`badge.text = "PROD"`, `badge.color = "red"`, `badge.position = "bottom-right"`
2. 渲染：在 `terminal_shader.rs` 中添加 badge 渲染通道
3. 支持 per-profile badge（不同 SSH 连接显示不同标识）
4. 透明度可调（避免遮挡终端内容）
