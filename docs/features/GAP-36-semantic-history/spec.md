---
gap: GAP-36
title: Semantic History
category: 开发者工具
plan_type: feature
priority: P2
effort: M
status: not-started
closed: null
---

# Semantic History（语义历史）

## 概述

Cmd-click 终端输出中的文件路径，自动在编辑器中打开对应文件和行号。支持多种编辑器（VS Code、Vim、Emacs）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | Cmd-click 打开文件在编辑器中（带行号），支持多种编辑器 | `sources/SemanticHistory/` (iTermSemanticHistoryController, iTermURLActionFactory, iTermPathFinder, iTermPathCleaner) |

## PureTerm 现状

**未实现。** PureTerm 的智能选择可检测文件路径，但没有 Cmd-click 打开编辑器的功能。

## 优先级

**P2 — 终端增强**

Semantic History 是开发者日常高频操作，从编译错误跳转到源码。

## 工作量

**M（1 周）**

需要：路径解析（文件名 + 行号）、编辑器检测、命令执行。

## 实现方向

1. 智能选择增强：检测 `file.rs:42` 格式的路径
2. Cmd-click：执行 `$EDITOR file.rs +42` 或 `code -g file.rs:42`
3. 编辑器检测：`$EDITOR` 环境变量 > 配置 > 默认 `code`
4. 路径解析：从编译器输出中提取（`error[E0308]: src/main.rs:42:5`）
5. 配置：`semantic_history.editor = "code"`, `semantic_history.enabled = true`
