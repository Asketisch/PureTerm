//! GAP-01: pane context injection + provider wiring for the chat panel.

use crate::ai_chat::types::{ChatMessage, ChatRequest, ChatRole};
use crate::app_config::AiChatConfig;
use crate::shell_context::build_for_pane;
use crate::shell_state::ShellState;
use alacritty_terminal::term::Term;
use crate::terminal_context::ChannelEventListener;

/// Max visible terminal lines attached to each user turn.
pub const CONTEXT_OUTPUT_LINES: usize = 80;

const SYSTEM_PROMPT: &str = "You are an AI assistant embedded in a terminal emulator. \
Help the user with shell commands, debugging, and code questions. Be concise. When suggesting \
commands, use markdown fenced code blocks tagged bash or sh.";

/// Build a human-readable context block from shell integration state + visible grid tail.
pub fn build_pane_context(
    shell_state: Option<&ShellState>,
    cwd: Option<&str>,
    term: Option<&Term<ChannelEventListener>>,
    lsp_diagnostics: Option<&str>,
) -> String {
    let shell_summary = build_for_pane(shell_state, cwd, 5);
    let mut parts = vec![shell_summary.to_prompt_string()];
    if let Some(term) = term {
        let snapshot =
            crate::surface_snapshot::capture_visible_tail(term, CONTEXT_OUTPUT_LINES);
        if !snapshot.is_empty() {
            parts.push(format!("Recent terminal output:\n```\n{snapshot}\n```"));
        }
    }
    if let Some(lsp) = lsp_diagnostics {
        let trimmed = lsp.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    parts.join("\n\n")
}

/// Build a streaming completion request from panel history + fresh context for the latest turn.
pub fn build_request(
    cfg: &AiChatConfig,
    messages: &[ChatMessage],
    context_block: &str,
    soul_extension: &str,
) -> ChatRequest {
    let mut api_messages: Vec<ChatMessage> = messages.to_vec();
    if let Some(last) = api_messages.last() {
        if last.role == ChatRole::Assistant && last.content.is_empty() {
            api_messages.pop();
        }
    }
    if let Some(last) = api_messages.last_mut() {
        if last.role == ChatRole::User && !context_block.is_empty() {
            let user_text = last.content.clone();
            last.content = format!(
                "[Terminal context]\n{context_block}\n\n[User message]\n{user_text}"
            );
        }
    }
    let system = if soul_extension.trim().is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{SYSTEM_PROMPT}\n\n{soul_extension}")
    };
    ChatRequest {
        model: cfg.model.clone(),
        system: Some(system),
        messages: api_messages,
        max_tokens: cfg.max_tokens,
        temperature: Some(0.7),
    }
}

/// Resolve a streaming LLM provider from config + environment.
pub fn make_provider(
    cfg: &AiChatConfig,
) -> Option<std::sync::Arc<dyn crate::ai_chat::provider::ChatProvider>> {
    use crate::ai_chat::anthropic::AnthropicProvider;
    use crate::ai_chat::openai::OpenAiProvider;
    use crate::ai_chat::provider::{resolve_key, ChatProvider};

    let client = reqwest::Client::new();
    let lookup = |name: &str| std::env::var(name).ok();

    match cfg.provider.as_str() {
        "openai" | "openai-compatible" => {
            let key = resolve_key(lookup, cfg.api_key_env.as_deref(), "OPENAI_API_KEY")?;
            Some(
                std::sync::Arc::new(OpenAiProvider::new(
                    client,
                    key,
                    cfg.openai_base_url.clone(),
                )) as std::sync::Arc<dyn ChatProvider>,
            )
        }
        _ => {
            let key = resolve_key(lookup, cfg.api_key_env.as_deref(), "ANTHROPIC_API_KEY")?;
            Some(
                std::sync::Arc::new(AnthropicProvider::new(client, key))
                    as std::sync::Arc<dyn ChatProvider>,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_pane_context_includes_lsp_diagnostics() {
        let block = build_pane_context(None, Some("/tmp"), None, Some("LSP diagnostics:\n- a.rs:1:1 error: x"));
        assert!(block.contains("LSP diagnostics"));
    }

    #[test]
    fn build_request_injects_context_into_last_user_turn() {
        let cfg = AiChatConfig::default();
        let messages = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi"),
            ChatMessage::user("what failed?"),
            ChatMessage::assistant(""),
        ];
        let req = build_request(&cfg, &messages, "cwd: /tmp", "");
        let last = req.messages.last().unwrap();
        assert!(last.content.contains("[Terminal context]"));
        assert!(last.content.contains("cwd: /tmp"));
        assert!(last.content.contains("what failed?"));
        assert_eq!(req.messages.len(), 3);
    }
}
