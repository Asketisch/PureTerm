---
gap: GAP-06
title: Ambient Agents
category: AI Agent
plan_type: feature
priority: P3
effort: XL
status: done
closed: 2026-06-29
---

# Ambient Agents（云端异步 AI 任务）

## 概述

AI agent 在云端异步执行任务（代码审查、issue triage、PR review），通过消息系统通知用户结果。支持多来源触发（CLI/Slack/GitHub/Linear）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Warp** | 云端异步 AI 任务，消息系统（SSE），多来源（CLI/Slack/GitHub/Linear） | `app/src/ai/ambient_agents/`, `app/src/ai/agent_sdk/ambient.rs` |
| **cmux** | Cloud VM + Web 平台，但非异步 agent 模式 | — |

## PureTerm 现状

**未实现。** PureTerm 的 Agent Hooks 是同步 relay 模式——agent 在本地终端执行，无云端异步能力。

## 优先级

**P2 — 终端增强**

Ambient Agents 是 Warp 的差异化功能，但需要云端基础设施。PureTerm 可作为长期目标。

## 工作量

**XL（3+ 周）**

需要：云端任务队列、SSE 推送、多来源 webhook、消息持久化、UI 通知面板。涉及后端服务。

## 实现方向

1. 纯客户端实现：通过 GitHub Actions / Linear webhook 触发本地 agent
2. 或：接入 Warp 的 Ambient Agent API（如果开放）
3. 消息通知：复用 Event System（GAP-6.7）+ macOS/Linux 通知
4. UI：侧边栏显示异步任务状态和结果
