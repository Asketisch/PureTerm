//! GAP-01: Built-in AI Chat Panel.
//!
//! PureTerm's first "active" AI surface — a dockable panel where the user chats
//! with an LLM (Anthropic Claude or any OpenAI-compatible endpoint), with the
//! active pane's recent output auto-injected as context and the assistant reply
//! streamed in as Markdown.
//!
//! This is the **first** HTTP/SSE/LLM/Markdown code in the repo; everything else
//! (panel state machine, streaming-into-GUI, config, persistence, CLI, Settings)
//! reuses established patterns. The provider/SSE layer here is greenfield.
//!
//! ## Layout
//! - [`sse`] — HTTP-agnostic incremental SSE byte decoder (pure, fully tested).
//! - [`types`] — `ChatRole`/`ChatMessage`/`ChatRequest`/`ChatEvent`.
//! - [`provider`] — `ChatProvider` trait + shared `run_sse` read loop + `SseMapping`.
//! - [`anthropic`] / [`openai`] — concrete providers + their SSE→`ChatEvent` mappers.
//! - [`markdown`] — `pulldown-cmark` → iced text spans.
//! - [`panel`] — `ChatPanelState` (open/messages/input/in-flight cancel).
//! - [`session`] — conversation persistence (`<data_dir>/chat/<id>.json`).
//!
//! Phased: Phase 1 ships `sse`/`types`/`provider`/`anthropic`/`openai` headless;
//! Phase 2 adds the panel + markdown + context injection; Phase 3 wires config /
//! persistence / CLI / Settings; Phase 4 (deferred) adds MCP function calling.

/// Incremental SSE byte decoder (HTTP-agnostic, pure std).
pub mod sse;
/// Core chat types: `ChatRole` / `ChatMessage` / `ChatRequest` / `ChatEvent`.
pub mod types;
/// `SseMapping` trait (provider SSE→`ChatEvent` translation). `ChatProvider` +
/// the shared `run_sse` read loop land here next (need stream glue).
pub mod provider;
/// Anthropic (Claude) SSE→`ChatEvent` mapper (`AnthropicMap`).
pub mod anthropic;
/// OpenAI-compatible SSE→`ChatEvent` mapper (`OpenAiMap`).
pub mod openai;
/// Markdown → structured blocks (`pulldown-cmark` → `Block`/`MarkdownSpan`).
pub mod markdown;
/// `ChatPanelState` — pure panel state machine (messages / gen / input).
pub mod panel;
/// Conversation persistence (`<data_dir>/chat/<id>.json`, atomic write).
pub mod session;
