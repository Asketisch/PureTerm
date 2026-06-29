//! `ChatPanelState` — the pure state machine for the dockable chat panel.
//!
//! Holds the conversation, the in-flight turn's generation id (so stale chunks
//! from a superseded/aborted turn are discarded), and the input draft. The state
//! machine itself is synchronous and HTTP-free: the app layer drives a provider's
//! stream and forwards each `ChatEvent` (tagged with the turn's gen) to
//! [`ChatPanelState::apply_chunk`]. Cancellation (panel close / new turn / Stop)
//! is app-layer — it drops the stream `Task`, and the provider's `cancel` signal
//! aborts the HTTP request.

use crate::ai_chat::types::{ChatEvent, ChatMessage, ChatRole};

/// Right-docked AI chat panel state. UI rendering + stream lifecycle live in the
/// app (`iced_app.rs`); this struct is the testable core.
#[derive(Debug)]
pub struct ChatPanelState {
    pub open: bool,
    pub messages: Vec<ChatMessage>,
    /// Draft text in the input box.
    pub input: String,
    /// The generation id of the turn currently streaming, if any.
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

    /// Abort the in-flight turn without waiting for a terminal stream event.
    pub fn stop_streaming(&mut self) {
        self.inflight_gen = None;
    }

    /// Begin a turn: push the user message + an empty assistant placeholder, and
    /// return a fresh generation id. The caller tags the provider stream's events
    /// with this id so stale chunks are discarded.
    pub fn begin_turn(&mut self, user_text: &str) -> u64 {
        self.messages.push(ChatMessage::user(user_text));
        self.messages.push(ChatMessage::assistant(String::new()));
        self.gen_counter += 1;
        self.inflight_gen = Some(self.gen_counter);
        self.gen_counter
    }

    /// Apply a streamed chunk for generation `gen`. Chunks whose gen doesn't match
    /// the in-flight turn are dropped (the turn was superseded/aborted).
    pub fn apply_chunk(&mut self, gen: u64, ev: &ChatEvent) {
        if Some(gen) != self.inflight_gen {
            return; // stale chunk from a superseded/aborted turn
        }
        match ev {
            ChatEvent::TextDelta(t) => {
                // Append to the in-flight assistant placeholder (the last message).
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
                // Mark the slot: replace an empty placeholder, or note after partial text.
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
            // Non-fatal warnings + Phase-4 tool calls: ignored by the state machine.
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
        assert!(p.is_streaming()); // not finalized yet
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
        let gen2 = p.begin_turn("second"); // supersedes gen1
        // A late chunk from gen1 must not touch the gen2 assistant placeholder.
        p.apply_chunk(gen1, &ChatEvent::TextDelta("ghost".into()));
        assert_eq!(p.messages.last().unwrap().content, "");
        // A current (gen2) chunk still applies.
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
}
