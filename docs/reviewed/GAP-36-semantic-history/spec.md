---
gap: GAP-36
title: Semantic History
category: 开发者工具
plan_type: feature
priority: P2
effort: M
status: done
closed: 2026-06-29
---

# Semantic History（语义历史）

## 概述

Cmd-click 终端输出中的文件路径，自动在编辑器中打开对应文件和行号。支持多种编辑器（VS Code、Vim、Emacs）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | Cmd-click 打开文件在编辑器中（带行号），支持多种编辑器 | `sources/SemanticHistory/` |

## PureTerm 现状

**已实现（2026-06-29）。** 扩展 GAP-4.2 hover_preview：`path:line[:col]` 解析（含 rustc `src/main.rs:42:5`）、⌘-click 打开编辑器、右键菜单「在编辑器中打开」、`OpenSemanticTarget` / `semantic_history.open` 动作。

## 优先级

**P2 — 终端增强**

## 工作量

**M（1 周）**

## 实现

- `crates/pureterm/src/hover_preview.rs` — `semantic_target_at_point` / `parse_path_line` / `resolve_editor` / `open_in_editor`（code `-g`、vim `+line`、emacs）
- `iced_app.rs` — `keyboard_modifiers` + `PaneMousePressed` ⌘-click + `OpenSemanticInEditor`
- `[semantic_history]` 配置：`enabled` / `editor`（`$EDITOR` > 配置 > `code`）
- 单测：`hover_preview::tests` + `app_config::semantic_history_parses_fields`
