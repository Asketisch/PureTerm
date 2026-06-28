---
gap: GAP-01
title: AI Chat Panel
category: AI Agent
plan_type: feature
priority: P0
effort: XL
status: in-progress
closed: null
---

# AI Chat Panel（内置 AI 聊天面板）

## 概述

在终端内嵌入一个 AI 聊天面板，用户可以通过自然语言与 LLM 交互，获取命令建议、代码解释、错误分析等。支持流式 Markdown 渲染、代码高亮、多模型切换。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Kaku** | `Cmd+L` 打开 Chat Panel，流式 Markdown + 代码高亮，终端上下文注入 | `Kaku-main/crates/kaku/src/chat/` |
| **Warp** | 内置 Oz agent，多 harness（Oz/Claude/Codex/Gemini），headless terminal pane 执行 | `app/src/ai/agent/`, `app/src/ai/agent_sdk/driver.rs` |
| **iTerm2** | AITerm：多 LLM provider（OpenAI/Anthropic/Gemini/DeepSeek），streaming SSE，function calling，vector store RAG | `sources/AITerm/` (LLM.swift, ChatViewController.swift, ChatWindowController.swift) |
| **cmux** | 无内置 Chat，依赖外部 agent | — |

## PureTerm 现状

**🚧 Phase 1+2 headless（库已提交，GUI 未接）。** `crates/pureterm/src/ai_chat/`（commit 449272e）含 provider/SSE/markdown/panel/session 层，**~71 单元测试**绿；`iced_app.rs` 零引用，无 `[ai_chat]` 配置，用户不可见。

**Phase 1+2 已交付：**
- `sse.rs` — HTTP 无关增量 SSE 字节解码器（纯函数，全测）
- `types.rs` — `ChatRole` / `ChatMessage` / `ChatRequest` / `ChatEvent`
- `provider.rs` + `anthropic.rs` + `openai.rs` — `ChatProvider` trait + Anthropic / OpenAI-compatible 流式 SSE
- `markdown.rs` — `pulldown-cmark` → structured blocks
- `panel.rs` — `ChatPanelState` 纯状态机（messages / in-flight cancel / input）
- `session.rs` — 对话持久化 `<data_dir>/chat/<id>.json`（原子写）

**仍缺（Phase 3+，不可标 ✅）：**
- `[ai_chat]` 配置 + Settings UI
- `iced_app` 面板 view + 快捷键（`⌘L` 与 Browser 地址栏冲突，需定方案）
- 活跃 pane 上下文注入（可复用 GAP-15 `get_shell_context` / surface snapshot）
- MCP function calling 联动（Phase 4 defer）

**既有「被动」AI 集成（不变）：**
- Agent Hooks — relay 到外部 agent
- MCP Server — 暴露终端工具给 agent
- Feed Approval — 审批面板

## 优先级

**P0 — AI 核心管道**

AI Chat Panel 是 AI-native 终端的门面功能，直接决定用户对"AI 终端"的第一印象。Kaku 和 Warp 都将其作为核心卖点。

## 工作量

**XL（2-3 周）**

Phase 1+2 headless 已落地；剩余 Phase 3 GUI/配置 + Phase 4 function calling。

## 实现方向

1. ✅ 新增 `crates/pureterm/src/ai_chat/` 模块
2. ✅ LLM provider trait + OpenAI / Anthropic 实现
3. 🚧 Chat UI：iced Panel widget（`ChatPanelState` 库就绪，未接 `iced_app`）
4. ❌ 上下文注入：自动附加当前 pane 的最近 N 行输出
5. ❌ 快捷键：打开/关闭 Chat Panel
6. ❌ 与 MCP Server 联动：Chat 可调用 MCP tools 实现 function calling
