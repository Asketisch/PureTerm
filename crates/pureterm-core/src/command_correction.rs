//! Command correction suggestions.
//!
//! When a command exits with a non-zero code, generate AI-powered suggestions
//! for what went wrong and how to fix it.

use crate::ai::{AiMessage, AiRole};

/// A command correction suggestion.
#[derive(Debug, Clone)]
pub struct Correction {
    /// The original (failed) command.
    pub original: String,
    /// Suggested corrected command.
    pub suggested: String,
    /// Brief explanation of what was wrong.
    pub explanation: String,
}

/// Build the prompt for command correction.
pub fn build_correction_prompt(
    command: &str,
    exit_code: i32,
    stderr_output: Option<&str>,
) -> Vec<AiMessage> {
    let system = AiMessage {
        role: AiRole::System,
        content: "You are a command-line expert. When a command fails, suggest the most likely fix. Be concise: one line explanation, one line corrected command. Reply in format: EXPLANATION: ...\nFIX: ...".into(),
    };

    let mut user_content = format!("Command '{}' failed with exit code {}.", command, exit_code);
    if let Some(stderr) = stderr_output {
        let truncated = if stderr.len() > 500 { &stderr[..500] } else { stderr };
        user_content.push_str(&format!("\nError output:\n{}", truncated));
    }

    vec![
        system,
        AiMessage {
            role: AiRole::User,
            content: user_content,
        },
    ]
}

/// Parse the AI response into a Correction.
pub fn parse_correction_response(original: &str, response: &str) -> Option<Correction> {
    let mut explanation = String::new();
    let mut suggested = String::new();

    for line in response.lines() {
        if let Some(rest) = line.strip_prefix("EXPLANATION:") {
            explanation = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("FIX:") {
            suggested = rest.trim().to_string();
        }
    }

    if explanation.is_empty() || suggested.is_empty() {
        return None;
    }

    Some(Correction {
        original: original.to_string(),
        suggested,
        explanation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_correction_prompt_no_stderr() {
        let messages = build_correction_prompt("git push", 1, None);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, AiRole::System);
        assert_eq!(messages[1].role, AiRole::User);
        assert!(messages[1].content.contains("git push"));
        assert!(messages[1].content.contains("exit code 1"));
    }

    #[test]
    fn test_build_correction_prompt_with_stderr() {
        let messages = build_correction_prompt(
            "ls /nonexistent",
            2,
            Some("No such file or directory"),
        );
        assert!(messages[1].content.contains("Error output:"));
        assert!(messages[1].content.contains("No such file or directory"));
    }

    #[test]
    fn test_build_correction_prompt_truncates_long_stderr() {
        let long_stderr = "x".repeat(1000);
        let messages = build_correction_prompt("cmd", 1, Some(&long_stderr));
        // The stderr in the user content should be truncated to 500 chars
        let user_msg = &messages[1].content;
        assert!(user_msg.len() < 600);
    }

    #[test]
    fn test_parse_correction_response_valid() {
        let response = "EXPLANATION: The remote branch does not exist.\nFIX: git push -u origin main";
        let correction = parse_correction_response("git push", response);
        assert!(correction.is_some());
        let c = correction.unwrap();
        assert_eq!(c.original, "git push");
        assert_eq!(c.suggested, "git push -u origin main");
        assert_eq!(c.explanation, "The remote branch does not exist.");
    }

    #[test]
    fn test_parse_correction_response_missing_fix() {
        let response = "EXPLANATION: Something went wrong.";
        let correction = parse_correction_response("cmd", response);
        assert!(correction.is_none());
    }

    #[test]
    fn test_parse_correction_response_missing_explanation() {
        let response = "FIX: sudo apt install foo";
        let correction = parse_correction_response("cmd", response);
        assert!(correction.is_none());
    }

    #[test]
    fn test_parse_correction_response_empty() {
        let correction = parse_correction_response("cmd", "");
        assert!(correction.is_none());
    }
}
