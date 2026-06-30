//! Anthropic (Claude) SSE → [`ChatEvent`](super::types::ChatEvent) mapping.
//!
//! Anthropic streams with **typed** SSE events (`event: <type>` plus a JSON
//! `data:` payload whose own `type` field echoes the event name). The events v1
//! text streaming cares about:
//!
//! ```text
//! event: message_start          data: {… "message":{"usage":{"input_tokens": N,…}}}
//! event: content_block_start    data: {…}                  (ignored in v1)
//! event: content_block_delta    data: {"delta":{"type":"text_delta","text":"…"}}
//! event: content_block_stop     data: {…}                  (ignored in v1)
//! event: message_delta          data: {"usage":{"output_tokens": N}}
//! event: message_stop           data: {…}   → terminal `Done`
//! ```
//!
//! `ping`, `content_block_start/stop`, and non-text content blocks (thinking,
//! tool_use — Phase 4) are ignored in v1.

use crate::ai_chat::provider::SseMapping;
use crate::ai_chat::sse::SseEvent;
use crate::ai_chat::types::ChatEvent;

/// Stateful Anthropic SSE→ChatEvent mapper. Construct one fresh per `stream()`
/// call; `input_tokens`/`output_tokens` accumulate across events and surface on
/// the terminal `message_stop`.
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
        // Malformed payload → skip (turn continues). Anthropic's payload `type`
        // field drives dispatch; we only care about a handful of event types.
        let data: serde_json::Value = serde_json::from_str(&ev.data).ok()?;
        match data.get("type").and_then(|t| t.as_str()) {
            // Assistant text delta. `delta.text` is absent on non-text content
            // blocks (thinking, tool_use — Phase 4); those fall through to None.
            Some("content_block_delta") => {
                let text = data.pointer("/delta/text")?.as_str()?;
                Some(ChatEvent::TextDelta(text.to_string()))
            }
            // Remember input_tokens for the final Done.
            Some("message_start") => {
                self.input_tokens = data
                    .pointer("/message/usage/input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                None
            }
            // Remember output_tokens (overrides the placeholder from message_start).
            Some("message_delta") => {
                self.output_tokens = data
                    .pointer("/usage/output_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                None
            }
            // Terminal: surface accumulated usage.
            Some("message_stop") => Some(ChatEvent::Done {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
            }),
            // content_block_start/stop, ping, unknown → ignored in v1.
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
        // message_start yields no event itself...
        assert_eq!(m.map(&start), None);
        // ...but input_tokens are remembered and surface on message_stop.
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
        assert_eq!(m.map(&md), None); // output_tokens remembered, no event yet
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
        // content_block_start/stop, ping, unknown types → no event in v1.
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
        // A truncated payload must not panic; it yields None and the turn continues.
        let bad = ev("content_block_delta", "{not valid json");
        assert_eq!(m.map(&bad), None);
    }
}

// ============================================================================
// AnthropicProvider — the reqwest HTTP shim around the mapper above.
// ============================================================================

use crate::ai_chat::provider::{classify_reqwest_error, drive_decode, ChatProvider, MpscReceiverStream, SecretKey};
use crate::ai_chat::types::ChatRequest;
use iced::futures::Stream;
use std::sync::Arc;

/// Anthropic Claude via the Messages API (`POST /v1/messages`, SSE response).
/// Holds a reqwest client + the env-resolved [`SecretKey`]; the model comes from
/// each [`ChatRequest`]. `derive(Debug)` is safe: [`SecretKey`]'s own `Debug`
/// redacts the key.
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

    /// Build the Messages API body. `system` is a top-level field (Anthropic
    /// models it separately from the message array); `temperature` only included
    /// when set.
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
            let mut resp = match resp {
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
            // drive_decode owns the body stream for the turn; resp stays alive in
            // this task for the duration of the await.
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
