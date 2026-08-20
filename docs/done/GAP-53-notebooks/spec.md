---
gap: GAP-53
title: 笔记本
category: 集成
plan_type: feature
priority: P4
effort: L
status: done
closed: 2026-07-02
---

# Notebooks（交互式文档）

## 概述

在终端内创建交互式文档——代码 cell + markdown + output，类似 Jupyter Notebook 但运行在终端中。支持 AI Document/Plan 能力。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | 交互式文档，代码 cell + markdown + output，AI Document/Plan | `app/src/notebooks/` |

## PureTerm 现状

**Done（2026-07-02）。** notebooks.rs .pureterm JSON 格式 + cell 执行引擎（$SHELL 调起） + 9 单测 + show/new/run CLI 子命令。

## 优先级

**P2 — 终端增强**

Notebooks 是 block 输出模型的差异化功能，但需要大量 UI 和执行引擎开发。

## 工作量

**L（1.5-2 周）**

需要：Cell 数据模型、代码执行引擎、markdown 渲染。

## 实现方向

1. Cell 类型：Code（执行 shell 命令）、Markdown（渲染文本）、Output（显示结果）
2. 执行引擎：每个 Code cell 独立 PTY
3. UI：Cell 列表 + 编辑/执行/删除
4. 保存：`.pureterm notebook` 格式（JSON）
5. AI 集成：AI Chat 可生成 notebook cells
