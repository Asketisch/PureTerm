---
gap: GAP-07
title: Soul System
category: AI Agent
plan_type: feature
priority: P2
effort: S
status: done
closed: 2026-06-29
---

# Soul System（AI 人格系统）

## 概述

通过配置文件定义 AI 的人格、风格、技能和记忆，使 AI 交互具有一致性和个性化。包含 SOUL.md（人格）、STYLE.md（风格）、SKILL.md（技能）、MEMORY.md（记忆）四个维度。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Kaku** | AI 人格文件（SOUL.md/STYLE.md/SKILL.md/MEMORY.md），启动时引导 LLM，自动策展 MEMORY.md | `Kaku-main/crates/kaku/src/soul/` |

## PureTerm 现状

**未实现。** PureTerm 的 Agent Hooks 仅提供 deny/ask 规则，无人格/记忆概念。

## 优先级

**P2 — 终端增强**

Soul System 提升 AI 交互的个性化体验，但非核心功能。可作为 AI Chat Panel（01）的增强。

## 工作量

**S（3-5 天）**

核心是配置文件解析 + prompt 注入。如果已有 AI Chat Panel，工作量很小。

## 实现方向

1. 配置目录：`~/.config/pureterm/ai/`
2. `SOUL.md`：AI 人格描述（如"你是一个 Rust 专家"）
3. `STYLE.md`：回复风格（简洁/详细/幽默）
4. `SKILL.md`：技能声明（会哪些语言/工具）
5. `MEMORY.md`：长期记忆（用户偏好、项目上下文）
6. 启动 AI Chat 时自动加载并注入 system prompt
7. MEMORY.md 由 AI 自动更新（用户确认）
