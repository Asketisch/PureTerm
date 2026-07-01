---
gap: GAP-30
title: Toolbelt
category: UI/外壳
plan_type: feature
priority: P3
effort: L
status: done
closed: 2026-06-29
---

# Toolbelt（工具栏）

## 概述

侧边栏常驻工具面板，提供开发者常用工具：命令历史、目录书签、后台任务、笔记、粘贴历史、Profile 列表、代码片段、捕获输出。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | 11 tools: command history, directories, jobs, named marks, notes, paste history, profiles, snippets, status, captured output, web view | `sources/Toolbelt/` (ToolCommandHistoryView, ToolDirectoriesView, ToolJobs, ToolNamedMarks, ToolNotes, ToolPasteHistory, ToolProfiles, iTermToolSnippets, ToolStatus, CapturedOutput, ToolWebView) |

## PureTerm 现状

**未实现。** PureTerm 的 Command Palette 提供命令搜索，Status Bar 显示状态信息，但没有常驻的工具面板。

## 优先级

**P3 — 兼容性/国际化**

Toolbelt 是 iTerm2 的特色功能，但大部分工具可通过 CLI 替代。

## 工作量

**L（1.5-2 周）**

需要：多个工具面板 widget + 侧边栏容器。

## 实现方向

1. 作为 Dual Sidebar（19）的右侧栏 mode
2. 工具列表：`toolbelt.tools = ["history", "directories", "snippets", "paste_history"]`
3. 每个工具独立的 iced widget
4. 命令历史：从 daemon 的 shell history 读取
5. 目录书签：用户配置的常用目录
6. 代码片段：用户预定义的文本模板
7. 粘贴历史：最近 N 次粘贴内容
