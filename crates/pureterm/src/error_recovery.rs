//! GAP-02: Error Recovery — LLM-powered fix suggestions when a command fails.
//!
//! Pure logic layer: failure context, prompt construction, fix-command extraction,
//! and panel state machine. The iced app wires shell marks → grid capture →
//! provider stream → overlay UI.

use crate::ai_chat::types::{ChatMessage, ChatRequest, ChatEvent, ChatRole};
use crate::app_config::ErrorRecoveryConfig;

/// One failed command worth analyzing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureContext {
    pub pane_id: u64,
    pub command: Option<String>,
    pub exit_code: i32,
    pub cwd: Option<String>,
    pub output_text: String,
}

/// Inline error-recovery card state (testable without iced).
#[derive(Debug)]
pub struct ErrorRecoveryState {
    pub open: bool,
    pub pane_id: Option<u64>,
    pub failure: Option<FailureContext>,
    pub suggestion: String,
    pub suggested_command: Option<String>,
    inflight_gen: Option<u64>,
    gen_counter: u64,
}

impl Default for ErrorRecoveryState {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorRecoveryState {
    pub fn new() -> Self {
        Self {
            open: false,
            pane_id: None,
            failure: None,
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

    /// Begin a new analysis turn; returns generation id for chunk tagging.
    pub fn begin_analysis(&mut self, failure: FailureContext) -> u64 {
        self.open = true;
        self.pane_id = Some(failure.pane_id);
        self.failure = Some(failure);
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

/// Gate automatic analysis on command failure.
pub fn should_auto_analyze(
    exit_code: Option<i32>,
    integrated: bool,
    cfg: &ErrorRecoveryConfig,
) -> bool {
    cfg.enabled
        && cfg.auto_analyze
        && integrated
        && exit_code.is_some_and(|c| c != 0)
}

const SYSTEM_PROMPT: &str = "You are a shell troubleshooting assistant embedded in a terminal. \
When the user runs a command that fails, explain briefly what went wrong and suggest exactly one \
fix. Put the suggested command in a single markdown fenced code block tagged bash, sh, or shell. \
Do not suggest destructive commands unless clearly necessary. Keep explanations concise.";

/// Build a one-shot LLM request from failure context + config.
pub fn build_prompt(ctx: &FailureContext, cfg: &ErrorRecoveryConfig) -> ChatRequest {
    let cmd = ctx
        .command
        .clone()
        .unwrap_or_else(|| "<unknown command>".to_string());
    let cwd = ctx
        .cwd
        .clone()
        .unwrap_or_else(|| "<unknown cwd>".to_string());
    let output = if ctx.output_text.is_empty() {
        "(no captured output)".to_string()
    } else {
        ctx.output_text.clone()
    };
    let user = format!(
        "The following command failed in my terminal.\n\n\
         cwd: {cwd}\n\
         command: `{cmd}`\n\
         exit code: {exit}\n\n\
         output:\n```\n{output}\n```\n\n\
         Explain the failure and provide one command to fix it in a ```bash code block.",
        exit = ctx.exit_code,
    );
    ChatRequest {
        model: cfg.model.clone(),
        system: Some(SYSTEM_PROMPT.to_string()),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: user,
        }],
        max_tokens: cfg.max_tokens,
        temperature: Some(0.2),
    }
}

/// Extract the first bash/sh/shell fenced code block from LLM markdown output.
pub fn extract_fix_command(markdown: &str) -> Option<String> {
    let lower = markdown.to_lowercase();
    for tag in ["```bash", "```sh", "```shell", "```zsh"] {
        if let Some(start) = lower.find(tag) {
            let content_start = start + tag.len();
            let rest = &markdown[content_start..];
            if let Some(end) = rest.find("```") {
                let cmd = rest[..end].trim();
                if !cmd.is_empty() {
                    return Some(cmd.to_string());
                }
            }
        }
    }
    // Fallback: any fenced block (first block wins).
    if let Some(start) = markdown.find("```") {
        let rest = &markdown[start + 3..];
        let content_start = rest.find('\n').map(|i| i + 1).unwrap_or(0);
        let body = &rest[content_start..];
        if let Some(end) = body.find("```") {
            let cmd = body[..end].trim();
            if !cmd.is_empty() {
                return Some(cmd.to_string());
            }
        }
    }
    None
}

/// Resolve a streaming LLM provider from config + environment. Returns `None`
/// when the API key env var is unset (caller should surface a card error).
pub fn make_provider(cfg: &ErrorRecoveryConfig) -> Option<std::sync::Arc<dyn crate::ai_chat::provider::ChatProvider>> {
    use crate::ai_chat::anthropic::AnthropicProvider;
    use crate::ai_chat::openai::OpenAiProvider;
    use crate::ai_chat::provider::{resolve_key, ChatProvider};

    let client = reqwest::Client::new();
    let lookup = |name: &str| std::env::var(name).ok();

    match cfg.provider.as_str() {
        "openai" | "openai-compatible" => {
            let key = resolve_key(
                lookup,
                cfg.api_key_env.as_deref(),
                "OPENAI_API_KEY",
            )?;
            Some(std::sync::Arc::new(OpenAiProvider::new(
                client,
                key,
                cfg.openai_base_url.clone(),
            )) as std::sync::Arc<dyn ChatProvider>)
        }
        _ => {
            let key = resolve_key(
                lookup,
                cfg.api_key_env.as_deref(),
                "ANTHROPIC_API_KEY",
            )?;
            Some(std::sync::Arc::new(AnthropicProvider::new(client, key))
                as std::sync::Arc<dyn ChatProvider>)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ErrorRecoveryConfig {
        ErrorRecoveryConfig {
            model: "test-model".into(),
            ..ErrorRecoveryConfig::default()
        }
    }

    #[test]
    fn should_auto_analyze_requires_nonzero_exit() {
        let c = ErrorRecoveryConfig::default();
        assert!(should_auto_analyze(Some(1), true, &c));
        assert!(!should_auto_analyze(Some(0), true, &c));
        assert!(!should_auto_analyze(None, true, &c));
        assert!(!should_auto_analyze(Some(1), false, &c));
        assert!(!should_auto_analyze(
            Some(1),
            true,
            &ErrorRecoveryConfig {
                enabled: false,
                ..ErrorRecoveryConfig::default()
            }
        ));
    }

    #[test]
    fn build_prompt_includes_command_output_and_exit() {
        let ctx = FailureContext {
            pane_id: 1,
            command: Some("ls /nope".into()),
            exit_code: 2,
            cwd: Some("/tmp".into()),
            output_text: "No such file".into(),
        };
        let req = build_prompt(&ctx, &cfg());
        assert_eq!(req.model, "test-model");
        assert!(req.system.as_ref().unwrap().contains("shell"));
        let user = &req.messages[0].content;
        assert!(user.contains("ls /nope"));
        assert!(user.contains("exit code: 2"));
        assert!(user.contains("No such file"));
        assert!(user.contains("/tmp"));
    }

    #[test]
    fn extract_fix_command_bash_block() {
        let md = "Try this:\n```bash\nmkdir -p /tmp/foo\n```\n";
        assert_eq!(
            extract_fix_command(md).as_deref(),
            Some("mkdir -p /tmp/foo")
        );
    }

    #[test]
    fn extract_fix_command_sh_block() {
        let md = "```sh\nexport PATH=/usr/local/bin:$PATH\n```";
        assert_eq!(
            extract_fix_command(md).as_deref(),
            Some("export PATH=/usr/local/bin:$PATH")
        );
    }

    #[test]
    fn extract_fix_command_none_when_missing() {
        assert!(extract_fix_command("no code here").is_none());
    }

    #[test]
    fn state_streams_and_extracts_on_done() {
        let mut s = ErrorRecoveryState::new();
        let failure = FailureContext {
            pane_id: 3,
            command: Some("false".into()),
            exit_code: 1,
            cwd: None,
            output_text: String::new(),
        };
        let gen = s.begin_analysis(failure);
        s.apply_chunk(gen, &ChatEvent::TextDelta("Use:\n```bash\necho ok\n```".into()));
        assert!(s.is_streaming());
        s.apply_chunk(
            gen,
            &ChatEvent::Done {
                input_tokens: None,
                output_tokens: None,
            },
        );
        assert!(!s.is_streaming());
        assert_eq!(s.suggested_command.as_deref(), Some("echo ok"));
    }

    #[test]
    fn stale_gen_ignored() {
        let mut s = ErrorRecoveryState::new();
        let gen = s.begin_analysis(FailureContext {
            pane_id: 1,
            command: None,
            exit_code: 1,
            cwd: None,
            output_text: String::new(),
        });
        s.begin_analysis(FailureContext {
            pane_id: 2,
            command: None,
            exit_code: 1,
            cwd: None,
            output_text: String::new(),
        });
        s.apply_chunk(gen, &ChatEvent::TextDelta("stale".into()));
        assert!(!s.suggestion.contains("stale"));
    }
}
