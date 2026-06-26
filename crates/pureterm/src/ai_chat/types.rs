//! Core chat types shared by the provider layer, the panel, and persistence.
//!
//! `ChatMessage` / `ChatRole` are serde (de)serializable: providers wrap them
//! into their own wire formats (Anthropic vs OpenAI) and the session store
//! round-trips them through JSON. `ChatRequest` is the in-memory ask handed to a
//! provider. `ChatEvent` is the unit a streaming provider yields back to the GUI
//! (in-memory only — no serde; it never crosses a serialization boundary).

use serde::{Deserialize, Serialize};

/// Conversational role of a message. Serializes to the lowercase wire form both
/// Anthropic and OpenAI expect (`"user"` / `"assistant"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

/// One turn in a conversation. `content` is plain text (no multimodal in v1);
/// providers that need structured parts wrap this into their own body shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }
}

/// A completion request handed to a [`super::provider::ChatProvider`]. Oldest
/// turn first; `system` is an optional top-level instruction (Anthropic models
/// it separately, OpenAI as a leading system message — each provider adapts).
#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
}

/// One event yielded by a streaming provider. A turn produces ≥1 `TextDelta`,
/// then exactly one terminal `Done` or `Error`, then the stream ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatEvent {
    /// Incremental assistant text; the panel appends to the in-flight assistant
    /// message and re-renders (throttled).
    TextDelta(String),
    /// Successful completion. Optional token usage for display.
    Done {
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
    },
    /// Non-fatal warning (e.g. a malformed SSE line that was skipped). Logged,
    /// never surfaced to the user.
    Warn(String),
    /// Fatal error; the stream ends after this. Surfaced as an error bubble.
    Error(String),
    /// Reserved for Phase 4 (MCP function calling). Never constructed in v1.
    ToolCallRequest {
        id: String,
        name: String,
        args_json: String,
    },
}

impl ChatEvent {
    /// `Done` or `Error` — the events after which a stream must terminate.
    pub fn is_terminal(&self) -> bool {
        matches!(self, ChatEvent::Done { .. } | ChatEvent::Error(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_role_serializes_to_lowercase_wire_form() {
        assert_eq!(serde_json::to_string(&ChatRole::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&ChatRole::Assistant).unwrap(),
            "\"assistant\"",
        );
    }

    #[test]
    fn chat_role_deserializes_from_lowercase_wire_form() {
        assert_eq!(
            serde_json::from_str::<ChatRole>("\"assistant\"").unwrap(),
            ChatRole::Assistant,
        );
    }

    #[test]
    fn chat_message_round_trips_through_json() {
        let msg = ChatMessage::user("fix the bug");
        let json = serde_json::to_string(&msg).unwrap();
        // Wire shape both providers expect: {"role":"user","content":"..."}.
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"fix the bug\""));
        let back: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn assistant_message_round_trips() {
        let msg = ChatMessage::assistant("running `ls`...");
        let back: ChatMessage =
            serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
        assert_eq!(back.role, ChatRole::Assistant);
        assert_eq!(back.content, "running `ls`...");
    }

    #[test]
    fn done_and_error_are_terminal() {
        assert!(ChatEvent::Done {
            input_tokens: Some(10),
            output_tokens: Some(5)
        }
        .is_terminal());
        assert!(ChatEvent::Error("network".into()).is_terminal());
        assert!(!ChatEvent::TextDelta("hi".into()).is_terminal());
        assert!(!ChatEvent::Warn("skipped line".into()).is_terminal());
    }
}
