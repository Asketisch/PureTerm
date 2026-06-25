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

**未实现。** PureTerm 当前的 AI 集成方式是：
- Agent Hooks（`crates/pureterm/src/agent_hooks.rs`）— relay 到外部 agent
- MCP Server（`crates/pureterm/src/mcp/server.rs`）— 暴露终端工具给 agent
- Feed Approval（`crates/pureterm/src/feed_panel.rs`）— 审批面板

这些都是"被动"集成（等待外部 agent 调用），缺少一个"主动"的内置 AI 交互入口。

## 优先级

**P0 — AI 核心管道**

AI Chat Panel 是 AI-native 终端的门面功能，直接决定用户对"AI 终端"的第一印象。Kaku 和 Warp 都将其作为核心卖点。

## 工作量

**XL（2-3 周）**

需要实现：LLM provider 抽象层、SSE streaming、Markdown 渲染（iced widget）、代码高亮、上下文注入、多模型切换 UI。

## 实现方向

1. 新增 `crates/pureterm/src/ai_chat/` 模块
2. LLM provider trait：`ChatProvider { stream_chat(messages) -> impl Stream<Item=ChatChunk> }`
3. 内置 provider：OpenAI API、Anthropic API（可扩展）
4. Chat UI：iced Panel widget，左侧历史列表，右侧对话流
5. 上下文注入：自动附加当前 pane 的最近 N 行输出
6. 快捷键：`Cmd+L` 打开/关闭 Chat Panel（类 Kaku）
7. 与 MCP Server 联动：Chat 可调用 MCP tools 实现 function calling
