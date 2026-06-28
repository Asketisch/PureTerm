---
gap: GAP-12
title: Annotations & Marks
category: 终端交互
plan_type: feature
priority: P2
effort: M
status: not-started
closed: null
---

# Annotations & Marks（注释与标记）

## 概述

**Annotations**：在终端行内附加注释（笔记），类似代码注释但附着在终端输出上。**Marks**：在终端输出中标记关键位置（shell prompt、命令输出、图片、错误等），支持快速跳转。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | **Annotations**: PTYAnnotation + PTYNoteViewController（行内笔记）。**Marks**: Shell prompt marks, image marks, captured output marks, block marks, button marks, fold marks, path marks | `sources/Annotations/` (PTYAnnotation.h/m, PTYNoteViewController.h/m), `sources/Marks/` (VT100ScreenMark.h/m, iTermImageMark.h/m, iTermCapturedOutputMark.h/m, BlockMark.swift, ButtonMark.swift, FoldMark.swift, PathMark.swift) |
| **cmux** | 无明确 Annotations/Marks | — |

## PureTerm 现状

**未实现。** PureTerm 的 Status Bar 显示 CWD/Git 等信息，但没有终端行级别的标注和标记能力。

## 优先级

**P2 — 终端增强**

Marks 是 Shell Integration 的自然延伸，Annotations 是知识管理的基础。两者结合可大幅提升终端内容的可导航性。

## 工作量

**M（1 周）**

Marks 可随 Shell Integration（15）一起实现，Annotations 需要额外的 UI 层。

## 实现方向

1. **Marks**：在 Grid 数据结构中为行附加 `MarkType` 枚举（Prompt, Command, Output, Error, Image）
2. 侧边栏 Marks 面板：列出所有标记，点击跳转
3. 快捷键：`Cmd+↑/↓` 跳转到上/下一个 mark
4. **Annotations**：在行数据中附加 `Option<String>` 注释
5. 右键菜单：添加注释 → 弹出输入框
6. 注释显示：行左侧小图标，hover 显示内容
