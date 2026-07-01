---
gap: GAP-58
title: Custom Theme & CSS
category: 视觉
plan_type: feature
priority: P4
effort: M
status: done
closed: 2026-06-29
---

# Custom Theme & CSS（自定义主题和 CSS）

## 概述

完整的主题系统——支持自定义配色方案、字体、间距、CSS 注入（针对 UI 组件）。社区主题共享。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | 完整 Theme API，自定义 CSS 注入，社区贡献配色方案 | `tabby-core/src/api/theme.ts`, `tabby-community-color-schemes/src/` |
| **Kaku** | 内置主题（Kaku Dark/Light），字体（JetBrains Mono），自动暗/亮切换 | `Kaku-main/crates/termwiz/src/color.rs` |

## PureTerm 现状

**部分实现。** PureTerm 的 Settings UI（`crates/pureterm-settings/src/pages/colors.rs`）支持 Alacritty 标准配色方案，有 Ferra 风格暖色暗色主题。

## 优先级

**P3 — 兼容性/国际化**

Alacritty 兼容的配色方案已能满足大部分需求，CSS 注入是锦上添花。

## 工作量

**M（1 周）**

在现有主题系统基础上扩展。

## 实现方向

1. 主题格式：TOML（兼容 Alacritty）+ PureTerm 扩展
2. UI 组件主题：`[theme.sidebar]`, `[theme.tab_bar]`, `[theme.status_bar]`
3. 字体配置：`[theme.font]` 支持连字、emoji 字体
4. 暗/亮切换：跟随 macOS 系统外观
5. 社区主题：GitHub 仓库 + CLI `pureterm theme install <name>`
6. CSS 注入：Iced 不原生支持 CSS，但可通过 widget 属性自定义
