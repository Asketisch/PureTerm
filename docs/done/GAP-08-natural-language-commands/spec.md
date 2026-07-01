---
gap: GAP-08
title: Natural Language Commands
category: AI Agent
plan_type: feature
priority: P2
effort: M
status: done
closed: 2026-06-29
---

# Natural Language Commands（自然语言命令）

## 概述

在终端提示符输入 `# <描述>` 自动生成对应的 shell 命令。例如 `# find all files larger than 10MB` → `find . -size +10M`。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Kaku** | `# <描述>` 在提示符生成 shell 命令 | `Kaku-main/crates/kaku/src/natural_language.rs` |
| **Warp** | AI agent 可生成命令，但无 `#` 快捷语法 | — |

## PureTerm 现状

**未实现。** PureTerm 的 Command Palette（`Cmd+Shift+P`）仅支持已有命令的模糊搜索，无自然语言生成能力。

## 优先级

**P2 — 终端增强**

自然语言命令是 AI 终端的便捷入口，比打开 Chat Panel 更轻量。Kaku 将其作为核心体验。

## 工作量

**M（1 周）**

需要：shell integration 检测 `#` 前缀、调用 LLM API、将生成的命令插入命令行。

## 实现方向

1. Shell integration 检测 `# ` 前缀的输入
2. 截取 `#` 后的文本，调用 LLM 生成 shell 命令
3. 将生成的命令回写到终端命令行（不自动执行）
4. 用户可编辑后按 Enter 执行
5. 快捷键：`Cmd+#` 在任何位置插入 `# ` 并激活
6. 与 AI Chat Panel（01）共享 LLM provider
