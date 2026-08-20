//! 由 provider 层、面板与持久化共享的核心聊天类型。
//!
//! `ChatMessage` / `ChatRole` 可 serde(反)序列化:provider 把它们包装成
//! 各自的线上格式(Anthropic 或 OpenAI),会话存储
//! 通过 JSON 往返。`ChatRequest` 是交给
//! provider 的内存请求。`ChatEvent` 是流式 provider 回给 GUI 的单位
//! (仅内存 —— 无 serde;它从不跨越序列化边界)。

use serde::{Deserialize, Serialize};

/// 消息的对话角色。序列化为小写线上形式,
/// 这是 Anthropic 与 OpenAI 都期望的(`"user"` / `"assistant"`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

/// 对话中的一轮。`content` 是纯文本(v1 无多模态);
/// 需要结构化部分的 provider 会把它包装成自己的消息体。
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

/// 交给 [`super::provider::ChatProvider`] 的补全请求。最旧的
/// 回合在前;`system` 是可选顶层指令(Anthropic 单独建模,
/// OpenAI 作为开头的系统消息 —— 由各 provider 自行适配)。
#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
}

/// 流式 provider 产出的事件。一轮会产生 ≥1 个 `TextDelta`,
/// 随后恰好一个终态 `Done` 或 `Error`,然后流结束。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatEvent {
    /// 增量助手文本;面板追加到进行中的助手
    /// 消息并重新渲染(限流)。
    TextDelta(String),
    /// 成功完成。可选的 token 用量,用于展示。
    Done {
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
    },
    /// 非致命警告(例如被跳过的一条畸形 SSE 行)。记录日志,
    /// 绝不展示给用户。
    Warn(String),
    /// 致命错误;此后流结束。以错误气泡展示。
    Error(String),
    /// 为 Phase 4(MCP 函数调用)预留。v1 中从不构造。
    ToolCallRequest {
        id: String,
        name: String,
        args_json: String,
    },
}

impl ChatEvent {
    /// `Done` 或 `Error` —— 此后流必须终止的事件。
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
        // 两个 provider 都期望的线上格式:{"role":"user","content":"..."}。
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
