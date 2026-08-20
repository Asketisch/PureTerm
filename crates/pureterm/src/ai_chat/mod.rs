//! GAP-01:内置 AI 聊天面板。
//!
//! PureTerm 第一个'主动式' AI 界面——一个可停靠面板,用户在其中与
//! LLM(Anthropic Claude 或任意 OpenAI 兼容端点)对话,自动注入
//! 活动面板的近期输出作为上下文,并将助手回复以 Markdown 流式渲染。
//! 流式渲染为 Markdown。
//!
//! 这是仓库中**第一处** HTTP/SSE/LLM/Markdown 代码;其余部分(面板状态机、
//! 流式写入 GUI、配置、持久化、CLI、设置)复用既有模式。此处的
//! provider/SSE 层是从零开始的全新实现。
//!
//! ## 布局
//! - [`sse`] — 与 HTTP 无关的增量 SSE 字节解码器(纯函数,已充分测试)。
//! - [`types`] — `ChatRole`/`ChatMessage`/`ChatRequest`/`ChatEvent`。
//! - [`provider`] — `ChatProvider` trait + 共享的 `run_sse` 读取循环 + `SseMapping`。
//! - [`anthropic`] / [`openai`] — 具体 provider 及其 SSE→`ChatEvent` 映射器。
//! - [`markdown`] — `pulldown-cmark` → iced 文本片段。
//! - [`panel`] — `ChatPanelState`(打开/消息/输入/飞行中取消)。
//! - [`session`] — 会话持久化(`<data_dir>/chat/<id>.json`)。
//! - [`context`] — 面板上下文注入 + GUI 面板的 provider 接线。
//!
//! 分阶段:Phase 1 提供 `sse`/`types`/`provider`/`anthropic`/`openai` 无头部分;
//! Phase 2 增加面板 + markdown + 上下文注入;Phase 3 接线配置 /
//! 持久化 / CLI / 设置;Phase 4(延后)增加 MCP 函数调用。

/// 增量 SSE 字节解码器(与 HTTP 无关,纯 std)。
pub mod sse;
/// 核心聊天类型:`ChatRole` / `ChatMessage` / `ChatRequest` / `ChatEvent`。
pub mod types;
/// `SseMapping` trait(provider SSE→`ChatEvent` 转换)。`ChatProvider` +
/// 共享的 `run_sse` 读取循环随后在此落地(需要流胶水)。
pub mod provider;
/// Anthropic(Claude)SSE→`ChatEvent` 映射器(`AnthropicMap`)。
pub mod anthropic;
/// OpenAI 兼容 SSE→`ChatEvent` 映射器(`OpenAiMap`)。
pub mod openai;
/// Markdown → 结构化块(`pulldown-cmark` → `Block`/`MarkdownSpan`)。
pub mod markdown;
/// `ChatPanelState` — 纯面板状态机(消息 / gen / 输入)。
pub mod panel;
/// 会话持久化(`<data_dir>/chat/<id>.json`,原子写入)。
pub mod session;
/// 面板上下文注入 + `ChatRequest` / provider 接线。
pub mod context;
