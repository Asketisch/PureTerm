//! OpenAI 兼容的 SSE → [`ChatEvent`](super::types::ChatEvent) 映射。
//!
//! OpenAI 及所有兼容端点(DeepSeek、OpenRouter、本地 Ollama 的
//! OpenAI 垫片)都以**无类型** SSE 输出:没有 `event:` 字段,只有
//! `chat.completion.chunk` 形状的数据块,以 `data: [DONE]` 结束。
//!
//! ```text
//! data: {"choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}   (首块,无内容)
//! data: {"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}   (内容块)
//! data: {"choices":[{"delta":{},"finish_reason":"stop"}]}   (终止块)
//! data: [DONE]                            → 终止 `Done`
//! ```
//!
//! `usage` 仅在调用方请求了
//! `stream_options.include_usage` 时才会出现(此时挂在一个带空
//! `choices` 数组的尾块上);我们会捕获它(若存在)并在终止的
//! `Done` 中呈现。

use crate::ai_chat::provider::SseMapping;
use crate::ai_chat::sse::SseEvent;
use crate::ai_chat::types::ChatEvent;

/// 有状态的 OpenAI 兼容 SSE→ChatEvent 映射器。每次 `stream()`
/// 调用构造一个全新的实例。`finished` 防止出现双重 `Done`
/// (`finish_reason:"stop"` 块后紧跟着 `[DONE]` 哨兵)时生效。
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
        // 已终止——`finish_reason` 块后跟随的 `[DONE]`。
        if self.finished {
            return None;
        }
        // 流终止哨兵(OpenAI 总是最后发送它)。
        if ev.data == "[DONE]" {
            self.finished = true;
            return Some(ChatEvent::Done {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
            });
        }
        // 畸形载荷 → 跳过(回合继续)。
        let data: serde_json::Value = match serde_json::from_str(&ev.data) {
            Ok(v) => v,
            Err(_) => return None,
        };
        // 可选 usage(仅在请求了 stream_options.include_usage 时存在)。
        // usage 块带空 choices 数组,挂在尾部事件上。
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
        // 内容增量(常见情形)。
        if let Some(content) = data
            .pointer("/choices/0/delta/content")
            .and_then(|v| v.as_str())
        {
            return Some(ChatEvent::TextDelta(content.to_string()));
        }
        // finish_reason 存在(且无内容)→ 终止。有些服务器
        // 先发这个再发 `[DONE]`;用 `finished` 防止双重 Done。
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
        // 首块仅 role 的增量,或其他形状 → 忽略。
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
        // 尾部的 [DONE] 不得再产生一个 Done。
        assert_eq!(m.map(&data("[DONE]")), None);
    }

    #[test]
    fn usage_chunk_is_captured_and_surfaced_on_done() {
        let mut m = OpenAiMap::new();
        // 先来一些内容。
        let c = data(r#"{"choices":[{"delta":{"content":"hi"},"finish_reason":null}]}"#);
        assert_eq!(m.map(&c), Some(ChatEvent::TextDelta("hi".into())));
        // 尾部 usage 块(空 choices)——捕获,不产生事件。
        let u = data(r#"{"choices":[],"usage":{"prompt_tokens":8,"completion_tokens":2}}"#);
        assert_eq!(m.map(&u), None);
        // [DONE] 携带已记住的 usage。
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
// OpenAiProvider — 任意 OpenAI 兼容端点的 reqwest 垫片。
// ============================================================================

use crate::ai_chat::provider::{classify_reqwest_error, drive_decode, ChatProvider, MpscReceiverStream, SecretKey};
use crate::ai_chat::types::ChatRequest;
use iced::futures::Stream;
use std::sync::Arc;

/// 校验配置的 OpenAI 兼容 `base_url`:允许 `https://*`(任意主机)
/// 及 `http://127.0.0.1` / `http://localhost` / `http://[::1]`(本地
/// Ollama 垫片);拒绝其余一切(对其他主机的明文 HTTP、
/// `file://` 等),防止配置错误或被篡改的配置把
/// Bearer 密钥及完整终端上下文重定向到任意端点;纯手写、
/// 零依赖(scheme/host 手工拆分)。
pub fn validate_base_url(url: &str) -> Result<(), &'static str> {
    let u = url.trim();
    let (scheme, rest) = u.split_once("://").ok_or("base_url must be http(s)")?;
    match scheme {
        "https" => Ok(()),
        "http" => {
            // 主机是第一个 '/' 之前的片段;去掉端口号,再处理
            // IPv6 方括号,使 `[::1]:11434` → `::1`。
            let host = rest.split('/').next().unwrap_or("");
            let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
            let host = host.trim_start_matches('[').trim_end_matches(']');
            if matches!(host, "127.0.0.1" | "localhost" | "::1") {
                Ok(())
            } else {
                Err("http base_url must point to localhost (use https for remote)")
            }
        }
        _ => Err("base_url must be http(s)"),
    }
}

/// 任意 OpenAI 兼容的 chat-completions 端点(OpenAI、DeepSeek、OpenRouter、
/// 本地 Ollama 的 OpenAI 垫片)。`base_url` 是 API 前缀(默认
/// `https://api.openai.com/v1`);请求时在后面拼 `/chat/completions`;认证为
/// `Authorization: Bearer <key>`。
#[derive(Debug)]
pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: SecretKey,
    api_url: String,
}

impl OpenAiProvider {
    const DEFAULT_BASE: &'static str = "https://api.openai.com/v1";

    /// `base_url`:API 前缀,可含 `/v1`。`None` → 使用 OpenAI 默认地址。
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

    /// 完整的 chat-completions URL(base + `/chat/completions`)。
    pub fn endpoint(&self) -> &str {
        &self.api_url
    }

    /// OpenAI chat-completions 请求体。`system` 会成为开头的 `system` 角色消息
    /// (OpenAI 没有顶层 system 字段);`temperature` 仅在设置时带上。
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
        // temperature 是 f32 → 用容差比较,而非精确相等。
        let t = body["temperature"].as_f64().unwrap();
        assert!((t - 0.7).abs() < 1e-5, "temperature was {t}");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "be brief");
        assert_eq!(msgs[1]["role"], "user");
        // OpenAI 没有顶层 system 字段。
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

    #[test]
    fn validate_base_url_accepts_https() {
        assert!(validate_base_url("https://api.openai.com/v1").is_ok());
        assert!(validate_base_url("https://api.deepseek.com").is_ok());
    }

    #[test]
    fn validate_base_url_accepts_local_http_for_ollama() {
        assert!(validate_base_url("http://127.0.0.1:11434/v1").is_ok());
        assert!(validate_base_url("http://localhost:11434").is_ok());
        assert!(validate_base_url("http://[::1]:11434").is_ok());
    }

    #[test]
    fn validate_base_url_rejects_non_local_http() {
        // 链路本地 / 元数据 / 内网主机必须被拒绝。
        assert!(validate_base_url("http://169.254.169.254").is_err());
        assert!(validate_base_url("http://internal.corp.example/v1").is_err());
        assert!(validate_base_url("http://8.8.8.8").is_err());
    }

    #[test]
    fn validate_base_url_rejects_non_http_schemes() {
        assert!(validate_base_url("file:///etc/passwd").is_err());
        assert!(validate_base_url("gopher://x").is_err());
        assert!(validate_base_url("not-a-url").is_err());
    }
}
