//! `ChatPanelState` — 可停靠聊天面板的纯状态机模块。
//!
//! 保存会话、进行中回合的生成 id(用于丢弃已中止/被取代回合的过期块),
//! 以及输入草稿。状态机本身是同步且不依赖 HTTP 的:应用层驱动提供方的
//! 流,并将每个 `ChatEvent`(带回合生成 id 标记)转发给
//! [`ChatPanelState::apply_chunk`]。取消(面板关闭 / 新回合 / 停止)
//! 属于应用层职责——它丢弃流 `Task`,提供方的 `cancel` 信号
//! 会中止 HTTP 请求。
//! 状态机不感知网络细节,全部中止动作皆在应用层完成。

use crate::ai_chat::types::{ChatEvent, ChatMessage, ChatRole};

/// 右侧停靠的 AI 聊天面板状态。界面渲染与流生命周期在应用
/// (`iced_app.rs`)中;本结构体是可供测试的核心。
#[derive(Debug)]
pub struct ChatPanelState {
    pub open: bool,
    pub messages: Vec<ChatMessage>,
    /// 输入框中的草稿文本。
    pub input: String,
    /// 当前正在流式输出的回合的生成 id(若有)。
    inflight_gen: Option<u64>,
    gen_counter: u64,
}

impl Default for ChatPanelState {
    fn default() -> Self {
        Self {
            open: false,
            messages: Vec::new(),
            input: String::new(),
            inflight_gen: None,
            gen_counter: 0,
        }
    }
}

impl ChatPanelState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn set_input(&mut self, text: impl Into<String>) {
        self.input = text.into();
    }

    pub fn is_streaming(&self) -> bool {
        self.inflight_gen.is_some()
    }

    /// 中止进行中的回合,无需等待终止流事件。
    pub fn stop_streaming(&mut self) {
        self.inflight_gen = None;
    }

    /// 将进行中的助手回合标记为用户已停止。空占位消息
    /// 变为 "(stopped)";部分流式回复追加
    /// "[stopped]" 提示,使中止可见(与 [`Self::apply_chunk`] 的
    /// `Error` 分支行为一致)。不修改 `inflight_gen`——由调用方通过
    /// [`Self::stop_streaming`] / `abort_inflight_chat` 清除。
    pub fn mark_stopped(&mut self) {
        if let Some(last) = self.messages.last_mut() {
            if last.role == ChatRole::Assistant {
                if last.content.is_empty() {
                    last.content = "(stopped)".to_string();
                } else {
                    last.content.push_str("\n[stopped]");
                }
            }
        }
    }

    /// 开始一个回合:推送用户消息 + 空的助手占位消息,并
    /// 返回新的生成 id。调用方用它标记提供方流的事件,
    /// 以便丢弃过期块。
    pub fn begin_turn(&mut self, user_text: &str) -> u64 {
        self.messages.push(ChatMessage::user(user_text));
        self.messages.push(ChatMessage::assistant(String::new()));
        self.gen_counter += 1;
        self.inflight_gen = Some(self.gen_counter);
        self.gen_counter
    }

    /// 应用生成 `gen` 的流式块。gen 不匹配
    /// 进行中回合的块会被丢弃(该回合已被取代或中止)。
    pub fn apply_chunk(&mut self, gen: u64, ev: &ChatEvent) {
        if Some(gen) != self.inflight_gen {
            return; // 来自已取代/中止回合的过期块
        }
        match ev {
            ChatEvent::TextDelta(t) => {
                // 追加到进行中的助手占位消息(最后一条消息)。
                if let Some(last) = self.messages.last_mut() {
                    if last.role == ChatRole::Assistant {
                        last.content.push_str(t);
                    }
                }
            }
            ChatEvent::Done { .. } => {
                self.inflight_gen = None;
            }
            ChatEvent::Error(msg) => {
                // 标记该槽位:空占位直接覆盖,或在部分文本后追加提示。
                if let Some(last) = self.messages.last_mut() {
                    if last.role == ChatRole::Assistant {
                        if last.content.is_empty() {
                            last.content = format!("[error: {msg}]");
                        } else {
                            last.content.push_str(&format!("\n[error: {msg}]"));
                        }
                    }
                }
                self.inflight_gen = None;
            }
            // 非致命警告 + 第 4 阶段工具调用:状态机忽略。
            ChatEvent::Warn(_) | ChatEvent::ToolCallRequest { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_closed_and_empty() {
        let p = ChatPanelState::new();
        assert!(!p.open);
        assert!(p.messages.is_empty());
        assert!(!p.is_streaming());
    }

    #[test]
    fn open_close_toggles() {
        let mut p = ChatPanelState::new();
        p.open();
        assert!(p.open);
        p.close();
        assert!(!p.open);
    }

    #[test]
    fn begin_turn_pushes_user_and_assistant_placeholder() {
        let mut p = ChatPanelState::new();
        let gen = p.begin_turn("hi");
        assert_eq!(gen, 1);
        assert_eq!(p.messages.len(), 2);
        assert_eq!(p.messages[0], ChatMessage::user("hi"));
        assert_eq!(p.messages[1], ChatMessage::assistant(""));
        assert!(p.is_streaming());
    }

    #[test]
    fn text_delta_appends_to_assistant_placeholder() {
        let mut p = ChatPanelState::new();
        let gen = p.begin_turn("hi");
        p.apply_chunk(gen, &ChatEvent::TextDelta("Hello".into()));
        p.apply_chunk(gen, &ChatEvent::TextDelta(" world".into()));
        assert_eq!(p.messages[1].content, "Hello world");
        assert!(p.is_streaming()); // 尚未终结
    }

    #[test]
    fn done_finalizes_turn() {
        let mut p = ChatPanelState::new();
        let gen = p.begin_turn("hi");
        p.apply_chunk(
            gen,
            &ChatEvent::Done {
                input_tokens: Some(1),
                output_tokens: Some(2),
            },
        );
        assert!(!p.is_streaming());
    }

    #[test]
    fn stale_chunk_from_superseded_turn_is_dropped() {
        let mut p = ChatPanelState::new();
        let gen1 = p.begin_turn("first");
        let gen2 = p.begin_turn("second"); // 取代 gen1
        // gen1 的迟到块不得影响 gen2 的助手占位消息。
        p.apply_chunk(gen1, &ChatEvent::TextDelta("ghost".into()));
        assert_eq!(p.messages.last().unwrap().content, "");
        // 当前(gen2)块仍然生效。
        p.apply_chunk(gen2, &ChatEvent::TextDelta("real".into()));
        assert_eq!(p.messages.last().unwrap().content, "real");
    }

    #[test]
    fn error_marks_the_assistant_slot_and_stops_streaming() {
        let mut p = ChatPanelState::new();
        let gen = p.begin_turn("hi");
        p.apply_chunk(gen, &ChatEvent::Error("network down".into()));
        assert!(!p.is_streaming());
        assert!(p.messages.last().unwrap().content.contains("network down"));
    }

    #[test]
    fn stop_after_partial_text_appends_marker() {
        let mut p = ChatPanelState::new();
        let gen = p.begin_turn("hi");
        p.apply_chunk(gen, &ChatEvent::TextDelta("partial reply".into()));
        p.mark_stopped();
        let content = p.messages.last().unwrap().content.clone();
        assert!(content.contains("partial reply"));
        assert!(content.contains("[stopped]"));
    }

    #[test]
    fn stop_with_empty_placeholder_shows_stopped() {
        let mut p = ChatPanelState::new();
        p.begin_turn("hi");
        p.mark_stopped();
        assert_eq!(p.messages.last().unwrap().content, "(stopped)");
    }

    #[test]
    fn stale_chunk_after_stop_is_dropped() {
        // stop_streaming() 清除 inflight_gen → 旧 gen 的迟到块
        // 不得写入(已隐藏的)助手占位消息。
        let mut p = ChatPanelState::new();
        let gen = p.begin_turn("hi");
        p.apply_chunk(gen, &ChatEvent::TextDelta("first".into()));
        p.stop_streaming();
        // 停止后,迟到块并发到达。
        p.apply_chunk(gen, &ChatEvent::TextDelta(" ghost".into()));
        let content = p.messages.last().unwrap().content.clone();
        assert_eq!(content, "first"); // 幽灵块未追加
    }
}
