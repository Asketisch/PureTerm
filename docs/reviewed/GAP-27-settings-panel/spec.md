---
gap: GAP-27
title: Settings Panel
category: UI/外壳
plan_type: feature
priority: P4
effort: M
status: done
closed: 2026-06-08
---

# Settings Panel（设置面板）

## 概述

独立的图形化设置面板，提供所有配置项的可视化编辑：外观、快捷键、SSH、AI、主题等。支持实时预览和格式保持写入。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | 完整 Preferences UI（XIB，多标签页） | `sources/Preferences/` |
| **参考实现 B** | Settings UI（Angular 组件） | — |
| **参考实现 C** | `Cmd + ,` 设置面板 | — |
| **参考实现 D** | Settings View（app 内） | `app/src/settings_view/` |

## PureTerm 现状

**已实现。** PureTerm 的 Settings UI（`crates/pureterm-settings/src/app.rs`）是独立 Iced 0.14 进程，6 个页面，格式保持写入。

## 优先级

**已实现**，无需新增。

## 备注

如果需要增强现有 Settings UI（如添加 AI Chat 配置、Trigger 配置页面），可作为现有功能的迭代。
