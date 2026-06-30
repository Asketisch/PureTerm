//! GAP-08: Natural language commands — `# description` → LLM shell command card.
//!
//! Reuses the GAP-02 error-recovery streaming pattern and `extract_fix_command`.

use crate::ai_chat::types::{ChatEvent, ChatMessage, ChatRequest, ChatRole};
use crate::app_config::{AiChatConfig, NaturalLanguageConfig};
use crate::error_recovery::extract_fix_command;

/// One natural-language command request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NlContext {
    pub pane_id: u64,
    pub description: String,
    pub cwd: Option<String>,
}

/// Inline natural-language command card state (testable without iced).
#[derive(Debug)]
pub struct NaturalLanguageState {
    pub open: bool,
    pub pane_id: Option<u64>,
    pub context: Option<NlContext>,
    pub suggestion: String,
    pub suggested_command: Option<String>,
    inflight_gen: Option<u64>,
    gen_counter: u64,
}

impl Default for NaturalLanguageState {
    fn default() -> Self {
        Self::new()
    }
}

impl NaturalLanguageState {
    pub fn new() -> Self {
        Self {
            open: false,
            pane_id: None,
            context: None,
            suggestion: String::new(),
            suggested_command: None,
            inflight_gen: None,
            gen_counter: 0,
        }
    }

    pub fn is_streaming(&self) -> bool {
        self.inflight_gen.is_some()
    }

    pub fn inflight_matches(&self, gen: u64) -> bool {
        self.inflight_gen == Some(gen)
    }

    pub fn begin_analysis(&mut self, ctx: NlContext) -> u64 {
        self.open = true;
        self.pane_id = Some(ctx.pane_id);
        self.context = Some(ctx);
        self.suggestion.clear();
        self.suggested_command = None;
        self.gen_counter += 1;
        self.inflight_gen = Some(self.gen_counter);
        self.gen_counter
    }

    pub fn apply_chunk(&mut self, gen: u64, ev: &ChatEvent) {
        if Some(gen) != self.inflight_gen {
            return;
        }
        match ev {
            ChatEvent::TextDelta(t) => self.suggestion.push_str(t),
            ChatEvent::Done { .. } => {
                self.inflight_gen = None;
                self.suggested_command = extract_fix_command(&self.suggestion);
            }
            ChatEvent::Error(msg) => {
                if self.suggestion.is_empty() {
                    self.suggestion = format!("[error: {msg}]");
                } else {
                    self.suggestion.push_str(&format!("\n[error: {msg}]"));
                }
                self.inflight_gen = None;
            }
            ChatEvent::Warn(_) | ChatEvent::ToolCallRequest { .. } => {}
        }
    }

    pub fn stop_streaming(&mut self) {
        self.inflight_gen = None;
        if !self.suggestion.is_empty() {
            self.suggested_command = extract_fix_command(&self.suggestion);
        }
    }

    pub fn dismiss(&mut self) {
        self.open = false;
        self.inflight_gen = None;
    }
}

const SYSTEM_PROMPT: &str = "You translate natural language descriptions into shell commands for a terminal user. \
Respond with exactly one command in a single markdown fenced code block tagged bash, sh, or shell. \
Keep explanations minimal; prefer safe, common flags. Do not suggest destructive commands unless clearly required.";

/// Parse `# description` from the current input line.
pub fn parse_description(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return None;
    }
    let rest = trimmed[1..].trim();
    if rest.is_empty() {
        return None;
    }
    Some(rest.to_string())
}

/// Build a one-shot LLM request from natural-language context.
pub fn build_prompt(ctx: &NlContext, ai_cfg: &AiChatConfig, nl_cfg: &NaturalLanguageConfig) -> ChatRequest {
    let cwd = ctx
        .cwd
        .clone()
        .unwrap_or_else(|| "<unknown cwd>".to_string());
    let user = format!(
        "cwd: {cwd}\n\n\
         Natural language request: {desc}\n\n\
         Provide one shell command in a ```bash code block.",
        desc = ctx.description,
    );
    ChatRequest {
        model: ai_cfg.model.clone(),
        system: Some(SYSTEM_PROMPT.to_string()),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: user,
        }],
        max_tokens: nl_cfg.max_tokens,
        temperature: Some(0.2),
    }
}

/// Resolve LLM provider from AI chat config (shared provider). Reuses the
/// caller-supplied `reqwest::Client` so connection pool / TLS sessions persist
/// across turns.
pub fn make_provider(
    cfg: &AiChatConfig,
    client: reqwest::Client,
) -> Option<std::sync::Arc<dyn crate::ai_chat::provider::ChatProvider>> {
    crate::ai_chat::context::make_provider_with_client(cfg, client)
}

/// Bytes to clear the current shell line (readline: home + kill to end).
pub const CLEAR_INPUT_LINE: &[u8] = b"\x01\x0b";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_description_requires_hash_prefix() {
        assert_eq!(
            parse_description("# find large files").as_deref(),
            Some("find large files")
        );
        assert!(parse_description("find files").is_none());
        assert!(parse_description("#").is_none());
        assert!(parse_description("#   ").is_none());
    }

    #[test]
    fn state_extracts_command_on_done() {
        let mut s = NaturalLanguageState::new();
        let ctx = NlContext {
            pane_id: 1,
            description: "list files".into(),
            cwd: Some("/tmp".into()),
        };
        let gen = s.begin_analysis(ctx);
        s.apply_chunk(
            gen,
            &ChatEvent::TextDelta("```bash\nls -la\n```".into()),
        );
        s.apply_chunk(
            gen,
            &ChatEvent::Done {
                input_tokens: None,
                output_tokens: None,
            },
        );
        assert_eq!(s.suggested_command.as_deref(), Some("ls -la"));
    }

    #[test]
    fn build_prompt_includes_description_and_cwd() {
        let ctx = NlContext {
            pane_id: 2,
            description: "disk usage".into(),
            cwd: Some("/home".into()),
        };
        let req = build_prompt(
            &ctx,
            &AiChatConfig::default(),
            &NaturalLanguageConfig::default(),
        );
        let user = &req.messages[0].content;
        assert!(user.contains("disk usage"));
        assert!(user.contains("/home"));
    }
}
