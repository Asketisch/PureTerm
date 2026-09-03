//! Anthropic(Claude)SSE → [`ChatEvent`](super::types::ChatEvent) 映射。
//!
//! Anthropic 采用**带类型**的 SSE 事件流输出(`event: <type>` 外加一个 JSON
//! `data:` 载荷,其自身的 `type` 字段与事件名一致)。v1 版
//! 文本流式输出所关注的事件如下:
//!
//! ```text
//! event: message_start          data: {… "message":{"usage":{"input_tokens": N,…}}}   (流开始,记录输入 token 数)
//! event: content_block_start    data: {…}                  (v1 中忽略)
//! event: content_block_delta    data: {"delta":{"type":"text_delta","text":"…"}}   (文本增量)
//! event: content_block_stop     data: {…}                  (v1 中忽略)
//! event: message_delta          data: {"usage":{"output_tokens": N}}   (记录输出 token 数)
//! event: message_stop           data: {…}   → 终止 `Done`
//! ```
//!
//! `ping`、`content_block_start/stop` 以及非文本内容块(thinking、
//! tool_use —— Phase 4)在 v1 中一律忽略。

use crate::ai_chat::provider::SseMapping;
use crate::ai_chat::sse::SseEvent;
use crate::ai_chat::types::ChatEvent;

/// 有状态的 Anthropic SSE→ChatEvent 映射器。每次 `stream()` 调用都构造全新实例;
/// `input_tokens`/`output_tokens` 跨事件累积,并在终止的
/// `message_stop` 上呈现出来。
#[derive(Debug, Default)]
pub struct AnthropicMap {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

impl AnthropicMap {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SseMapping for AnthropicMap {
    fn map(&mut self, ev: &SseEvent) -> Option<ChatEvent> {
        // 畸形载荷 → 跳过(回合继续)。Anthropic 载荷的 `type`
        // 字段驱动分发;我们只关心少数事件类型。
        let data: serde_json::Value = serde_json::from_str(&ev.data).ok()?;
        match data.get("type").and_then(|t| t.as_str()) {
            // 助手文本增量。`delta.text` 在非文本内容块
            // (thinking、tool_use —— Phase 4)上缺失;这些落入 None。
            Some("content_block_delta") => {
                let text = data.pointer("/delta/text")?.as_str()?;
                Some(ChatEvent::TextDelta(text.to_string()))
            }
            // 记住 input_tokens,供最终 Done 使用。
            Some("message_start") => {
                self.input_tokens = data
                    .pointer("/message/usage/input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                None
            }
            // 记住 output_tokens(覆盖 message_start 中的占位值)。
            Some("message_delta") => {
                self.output_tokens = data
                    .pointer("/usage/output_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                None
            }
            // 终止:呈现累积的用量。
            Some("message_stop") => Some(ChatEvent::Done {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
            }),
            // content_block_start/stop、ping、未知类型 → v1 中忽略。
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(event: &str, data: &str) -> SseEvent {
        SseEvent {
            event: Some(event.to_string()),
            data: data.to_string(),
        }
    }

    #[test]
    fn text_delta_is_extracted_from_content_block_delta() {
        let mut m = AnthropicMap::new();
        let e = ev(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
        );
        assert_eq!(m.map(&e), Some(ChatEvent::TextDelta("Hello".into())));
    }

    #[test]
    fn consecutive_text_deltas_stream_independently() {
        let mut m = AnthropicMap::new();
        let d1 = ev(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello "}}"#,
        );
        let d2 = ev(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}"#,
        );
        assert_eq!(m.map(&d1), Some(ChatEvent::TextDelta("Hello ".into())));
        assert_eq!(m.map(&d2), Some(ChatEvent::TextDelta("world".into())));
    }

    #[test]
    fn message_start_remembers_input_tokens_for_final_done() {
        let mut m = AnthropicMap::new();
        let start = ev(
            "message_start",
            r#"{"type":"message_start","message":{"usage":{"input_tokens":25,"output_tokens":1}}}"#,
        );
        // message_start 本身不产生事件……
        assert_eq!(m.map(&start), None);
        // ……但 input_tokens 已被记住,并在 message_stop 上呈现。
        let stop = ev("message_stop", r#"{"type":"message_stop"}"#);
        assert_eq!(
            m.map(&stop),
            Some(ChatEvent::Done {
                input_tokens: Some(25),
                output_tokens: None,
            }),
        );
    }

    #[test]
    fn full_turn_emits_deltas_then_done_with_usage() {
        let mut m = AnthropicMap::new();
        let start = ev(
            "message_start",
            r#"{"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":1}}}"#,
        );
        let delta = ev(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
        );
        let md = ev(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}"#,
        );
        let stop = ev("message_stop", r#"{"type":"message_stop"}"#);

        assert_eq!(m.map(&start), None);
        assert_eq!(m.map(&delta), Some(ChatEvent::TextDelta("hi".into())));
        assert_eq!(m.map(&md), None); // output_tokens 已记忆,但尚未产生事件
        assert_eq!(
            m.map(&stop),
            Some(ChatEvent::Done {
                input_tokens: Some(10),
                output_tokens: Some(5),
            }),
        );
    }

    #[test]
    fn ignored_events_yield_nothing() {
        let mut m = AnthropicMap::new();
        // content_block_start/stop、ping、未知类型 → v1 中不产生事件。
        assert_eq!(
            m.map(&ev(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
            )),
            None
        );
        assert_eq!(
            m.map(&ev("content_block_stop", r#"{"type":"content_block_stop","index":0}"#)),
            None
        );
        assert_eq!(m.map(&ev("ping", r#"{"type":"ping"}"#)), None);
    }

    #[test]
    fn malformed_json_payload_is_skipped_not_fatal() {
        let mut m = AnthropicMap::new();
        // 截断的载荷必须不 panic;它产生 None,回合继续。
        let bad = ev("content_block_delta", "{not valid json");
        assert_eq!(m.map(&bad), None);
    }
}

// ============================================================================
// AnthropicProvider — 上文映射器的 reqwest HTTP 垫片。
// ============================================================================

use crate::ai_chat::provider::{classify_reqwest_error, drive_decode, ChatProvider, MpscReceiverStream, SecretKey};
use crate::ai_chat::types::ChatRequest;
use iced::futures::Stream;
use std::sync::Arc;

/// Anthropic Claude,通过 Messages API(`POST /v1/messages`,SSE 响应)。
/// 持有 reqwest 客户端和从环境变量解析的 [`SecretKey`];模型来自
/// 每次 [`ChatRequest`]。`derive(Debug)` 是安全的:[`SecretKey`] 自身的 `Debug`
/// 实现会隐去密钥。
#[derive(Debug)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: SecretKey,
    api_url: String,
}

impl AnthropicProvider {
    const DEFAULT_URL: &'static str = "https://api.anthropic.com/v1/messages";

    pub fn new(client: reqwest::Client, api_key: SecretKey) -> Self {
        Self {
            client,
            api_key,
            api_url: Self::DEFAULT_URL.to_string(),
        }
    }

    /// 构造 Messages API 请求体。`system` 是顶层字段(Anthropic
    /// 把它与消息数组分开建模);`temperature` 仅在
    /// 设置时才包含。
    pub fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "stream": true,
            "messages": req
                .messages
                .iter()
                .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
                .collect::<Vec<_>>(),
        });
        if let Some(sys) = &req.system {
            body["system"] = serde_json::Value::String(sys.clone());
        }
        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        body
    }
}

impl ChatProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn stream(
        &self,
        req: ChatRequest,
        cancel: Arc<tokio::sync::Notify>,
    ) -> std::pin::Pin<Box<dyn Stream<Item = ChatEvent> + Send>> {
        let body = self.build_body(&req);
        let client = self.client.clone();
        let url = self.api_url.clone();
        let key = self.api_key.get().to_string();
        let (tx, rx) = tokio::sync::mpsc::channel::<ChatEvent>(64);
        tokio::spawn(async move {
            let resp = client
                .post(&url)
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await;
            let resp = match resp {
                Ok(r) if r.status().is_success() => r,
                Ok(r) => {
                    let _ = tx
                        .send(ChatEvent::Error(format!("HTTP {}", r.status())))
                        .await;
                    return;
                }
                Err(e) => {
                    let _ = tx
                        .send(ChatEvent::Error(format!(
                            "request failed: {}",
                            classify_reqwest_error(&e)
                        )))
                        .await;
                    return;
                }
            };
            // drive_decode 持有本回合的响应体流;resp 在此任务中
            // 存活至 await 结束。
            drive_decode(resp.bytes_stream(), AnthropicMap::new(), cancel, tx).await;
        });
        Box::pin(MpscReceiverStream::new(rx))
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::ai_chat::types::{ChatMessage, ChatRole};

    fn provider() -> AnthropicProvider {
        AnthropicProvider::new(reqwest::Client::new(), SecretKey::new("test-key"))
    }

    #[test]
    fn build_body_shape_matches_messages_api() {
        let req = ChatRequest {
            model: "claude-3-5-sonnet".into(),
            system: Some("be brief".into()),
            messages: vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
            max_tokens: 1024,
            temperature: Some(0.5),
        };
        let body = provider().build_body(&req);
        assert_eq!(body["model"], "claude-3-5-sonnet");
        assert_eq!(body["max_tokens"], 1024);
        assert_eq!(body["stream"], true);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["temperature"], 0.5);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "hi");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "hello");
    }

    #[test]
    fn build_body_omits_optional_fields_when_absent() {
        let req = ChatRequest {
            model: "claude-3-5-sonnet".into(),
            system: None,
            messages: vec![ChatMessage::user("ping")],
            max_tokens: 256,
            temperature: None,
        };
        let body = provider().build_body(&req);
        assert!(body.get("system").is_none());
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn debug_does_not_leak_the_api_key() {
        let p = provider();
        let dbg = format!("{:?}", p);
        assert!(!dbg.contains("test-key"));
    }

    #[test]
    fn id_is_anthropic() {
        assert_eq!(provider().id(), "anthropic");
    }
}
