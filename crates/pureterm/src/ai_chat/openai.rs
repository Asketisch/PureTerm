//! OpenAI-compatible SSE → [`ChatEvent`](super::types::ChatEvent) mapping.
//!
//! OpenAI and every compatible endpoint (DeepSeek, OpenRouter, local Ollama's
//! OpenAI shim) stream **untyped** SSE: no `event:` field, just `data:` JSON
//! chunks of the `chat.completion.chunk` shape, terminated by `data: [DONE]`.
//!
//! ```text
//! data: {"choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}   (first chunk, no content)
//! data: {"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}
//! data: {"choices":[{"delta":{},"finish_reason":"stop"}]}
//! data: [DONE]                            → terminal `Done`
//! ```
//!
//! `usage` appears only when the caller requested
//! `stream_options.include_usage` (then it rides a trailing chunk with an empty
//! `choices` array); we capture it if present and surface it on the terminal
//! `Done`.

use crate::ai_chat::provider::SseMapping;
use crate::ai_chat::sse::SseEvent;
use crate::ai_chat::types::ChatEvent;

/// Stateful OpenAI-compatible SSE→ChatEvent mapper. Construct one fresh per
/// `stream()` call. `finished` guards against a double `Done` (a
/// `finish_reason:"stop"` chunk followed by the `[DONE]` sentinel).
#[derive(Debug, Default)]
pub struct OpenAiMap {
    finished: bool,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

impl OpenAiMap {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SseMapping for OpenAiMap {
    fn map(&mut self, ev: &SseEvent) -> Option<ChatEvent> {
        // Already terminated — a trailing [DONE] after a finish_reason chunk.
        if self.finished {
            return None;
        }
        // Stream terminator sentinel (OpenAI always sends this last).
        if ev.data == "[DONE]" {
            self.finished = true;
            return Some(ChatEvent::Done {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
            });
        }
        // Malformed payload → skip (turn continues).
        let data: serde_json::Value = match serde_json::from_str(&ev.data) {
            Ok(v) => v,
            Err(_) => return None,
        };
        // Optional usage (only present with stream_options.include_usage). The
        // usage chunk has an empty choices array and rides the trailing event.
        if let Some(usage) = data.get("usage") {
            self.input_tokens = usage
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            self.output_tokens = usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
        }
        // Content delta (the common case).
        if let Some(content) = data
            .pointer("/choices/0/delta/content")
            .and_then(|v| v.as_str())
        {
            return Some(ChatEvent::TextDelta(content.to_string()));
        }
        // finish_reason present (and no content) → terminal. Some servers send
        // this then [DONE]; guard with `finished` to avoid a double Done.
        if data
            .pointer("/choices/0/finish_reason")
            .and_then(|v| v.as_str())
            .is_some()
        {
            self.finished = true;
            return Some(ChatEvent::Done {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
            });
        }
        // First-chunk role-only delta, or any other shape → ignored.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(data: &str) -> SseEvent {
        SseEvent {
            event: None,
            data: data.to_string(),
        }
    }

    #[test]
    fn content_delta_is_extracted() {
        let mut m = OpenAiMap::new();
        let e = data(r#"{"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}"#);
        assert_eq!(m.map(&e), Some(ChatEvent::TextDelta("Hello".into())));
    }

    #[test]
    fn done_sentinel_terminates() {
        let mut m = OpenAiMap::new();
        assert_eq!(
            m.map(&data("[DONE]")),
            Some(ChatEvent::Done {
                input_tokens: None,
                output_tokens: None,
            }),
        );
    }

    #[test]
    fn finish_reason_chunk_terminates_and_trailing_done_is_ignored() {
        let mut m = OpenAiMap::new();
        let stop = data(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        assert_eq!(
            m.map(&stop),
            Some(ChatEvent::Done {
                input_tokens: None,
                output_tokens: None,
            }),
        );
        // The trailing [DONE] must not emit a second Done.
        assert_eq!(m.map(&data("[DONE]")), None);
    }

    #[test]
    fn usage_chunk_is_captured_and_surfaced_on_done() {
        let mut m = OpenAiMap::new();
        // Some content first.
        let c = data(r#"{"choices":[{"delta":{"content":"hi"},"finish_reason":null}]}"#);
        assert_eq!(m.map(&c), Some(ChatEvent::TextDelta("hi".into())));
        // Trailing usage chunk (empty choices) — capture, no event.
        let u = data(r#"{"choices":[],"usage":{"prompt_tokens":8,"completion_tokens":2}}"#);
        assert_eq!(m.map(&u), None);
        // [DONE] carries the remembered usage.
        assert_eq!(
            m.map(&data("[DONE]")),
            Some(ChatEvent::Done {
                input_tokens: Some(8),
                output_tokens: Some(2),
            }),
        );
    }

    #[test]
    fn first_chunk_with_role_only_yields_nothing() {
        let mut m = OpenAiMap::new();
        let e = data(r#"{"choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}"#);
        assert_eq!(m.map(&e), None);
    }

    #[test]
    fn malformed_json_payload_is_skipped_not_fatal() {
        let mut m = OpenAiMap::new();
        assert_eq!(m.map(&data("{broken")), None);
    }
}

// ============================================================================
// OpenAiProvider — reqwest shim for any OpenAI-compatible endpoint.
// ============================================================================

use crate::ai_chat::provider::{drive_decode, ChatProvider, MpscReceiverStream, SecretKey};
use crate::ai_chat::types::ChatRequest;
use iced::futures::Stream;
use std::sync::Arc;

/// Any OpenAI-compatible chat-completions endpoint (OpenAI, DeepSeek, OpenRouter,
/// local Ollama's OpenAI shim). `base_url` is the API prefix (default
/// `https://api.openai.com/v1`); `/chat/completions` is appended. Auth is
/// `Authorization: Bearer <key>`.
#[derive(Debug)]
pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: SecretKey,
    api_url: String,
}

impl OpenAiProvider {
    const DEFAULT_BASE: &'static str = "https://api.openai.com/v1";

    /// `base_url`: API prefix including any `/v1`. `None` → OpenAI default.
    pub fn new(client: reqwest::Client, api_key: SecretKey, base_url: Option<String>) -> Self {
        let base = base_url
            .unwrap_or_else(|| Self::DEFAULT_BASE.to_string())
            .trim_end_matches('/')
            .to_string();
        Self {
            client,
            api_key,
            api_url: format!("{base}/chat/completions"),
        }
    }

    /// Full chat-completions URL (base + `/chat/completions`).
    pub fn endpoint(&self) -> &str {
        &self.api_url
    }

    /// OpenAI chat-completions body. `system` becomes a leading `system` role
    /// message (OpenAI has no top-level system field); `temperature` only when set.
    pub fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = Vec::new();
        if let Some(sys) = &req.system {
            messages.push(serde_json::json!({ "role": "system", "content": sys }));
        }
        for m in &req.messages {
            messages.push(serde_json::json!({ "role": m.role, "content": m.content }));
        }
        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "stream": true,
            "messages": messages,
        });
        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        body
    }
}

impl ChatProvider for OpenAiProvider {
    fn id(&self) -> &'static str {
        "openai"
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
                .bearer_auth(&key)
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
                        .send(ChatEvent::Error(format!("request failed: {e}")))
                        .await;
                    return;
                }
            };
            drive_decode(resp.bytes_stream(), OpenAiMap::new(), cancel, tx).await;
        });
        Box::pin(MpscReceiverStream::new(rx))
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::ai_chat::types::ChatMessage;

    fn provider() -> OpenAiProvider {
        OpenAiProvider::new(reqwest::Client::new(), SecretKey::new("test-key"), None)
    }

    #[test]
    fn build_body_system_becomes_leading_message() {
        let req = ChatRequest {
            model: "gpt-4o".into(),
            system: Some("be brief".into()),
            messages: vec![ChatMessage::user("hi")],
            max_tokens: 512,
            temperature: Some(0.7),
        };
        let body = provider().build_body(&req);
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["stream"], true);
        // temperature is f32 → compare with tolerance, not exact equality.
        let t = body["temperature"].as_f64().unwrap();
        assert!((t - 0.7).abs() < 1e-5, "temperature was {t}");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "be brief");
        assert_eq!(msgs[1]["role"], "user");
        // OpenAI has no top-level system field.
        assert!(body.get("system").is_none());
    }

    #[test]
    fn build_body_without_system_has_no_system_message() {
        let req = ChatRequest {
            model: "gpt-4o".into(),
            system: None,
            messages: vec![ChatMessage::user("hi")],
            max_tokens: 256,
            temperature: None,
        };
        let body = provider().build_body(&req);
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn default_endpoint_is_openai() {
        assert_eq!(
            provider().endpoint(),
            "https://api.openai.com/v1/chat/completions",
        );
    }

    #[test]
    fn custom_base_url_appends_chat_completions() {
        let p = OpenAiProvider::new(
            reqwest::Client::new(),
            SecretKey::new("k"),
            Some("https://api.deepseek.com".into()),
        );
        assert_eq!(p.endpoint(), "https://api.deepseek.com/chat/completions");
    }

    #[test]
    fn debug_does_not_leak_the_api_key() {
        let p = provider();
        assert!(!format!("{:?}", p).contains("test-key"));
    }

    #[test]
    fn id_is_openai() {
        assert_eq!(provider().id(), "openai");
    }
}
