//! Secret detection and redaction for terminal output.
//!
//! Detects common secret patterns (API keys, passwords, tokens) and marks them for visual redaction.

use once_cell::sync::Lazy;
use regex::Regex;

/// A detected secret in terminal output.
#[derive(Debug, Clone)]
pub struct DetectedSecret {
    /// Start column of the secret.
    pub start: usize,
    /// End column (exclusive) of the secret.
    pub end: usize,
    /// The type of secret detected.
    pub kind: SecretKind,
}

/// Types of detectable secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// AWS Access Key (AKIA...)
    AwsAccessKey,
    /// AWS Secret Key (40-char base64)
    AwsSecretKey,
    /// GitHub Token (ghp_, gho_, ghu_, ghs_, github_pat_)
    GitHubToken,
    /// Generic API key patterns
    ApiKey,
    /// Bearer token
    BearerToken,
    /// Private key header
    PrivateKey,
    /// Password in URL (password@host)
    PasswordInUrl,
}

/// Patterns for secret detection.
static SECRET_PATTERNS: Lazy<Vec<(Regex, SecretKind)>> = Lazy::new(|| {
    vec![
        (
            Regex::new(r"(?i)AKIA[0-9A-Z]{16}").unwrap(),
            SecretKind::AwsAccessKey,
        ),
        (
            Regex::new(r"(?i)aws_secret_access_key\s*=\s*[A-Za-z0-9/+=]{40}").unwrap(),
            SecretKind::AwsSecretKey,
        ),
        (
            Regex::new(r"gh[psou]_[A-Za-z0-9_]{36,255}").unwrap(),
            SecretKind::GitHubToken,
        ),
        (
            Regex::new(r"github_pat_[A-Za-z0-9_]{22,255}").unwrap(),
            SecretKind::GitHubToken,
        ),
        (
            Regex::new(r"(?i)bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap(),
            SecretKind::BearerToken,
        ),
        (
            Regex::new(r"-----BEGIN (?:RSA |EC |DSA )?PRIVATE KEY-----").unwrap(),
            SecretKind::PrivateKey,
        ),
        (
            Regex::new(r"(?i)://[^/\s:]+:([^@\s]{8,})@").unwrap(),
            SecretKind::PasswordInUrl,
        ),
        (
            Regex::new(r"(?i)(?:api[_-]?key|apikey|token|secret|password|passwd)\s*[=:]\s*\S{20,}")
                .unwrap(),
            SecretKind::ApiKey,
        ),
    ]
});

/// Detect secrets in a line of text. Returns ranges that should be redacted.
pub fn detect_secrets(line: &str) -> Vec<DetectedSecret> {
    let mut secrets = Vec::new();
    for (regex, kind) in SECRET_PATTERNS.iter() {
        for m in regex.find_iter(line) {
            secrets.push(DetectedSecret {
                start: m.start(),
                end: m.end(),
                kind: *kind,
            });
        }
    }
    // Sort by start position, remove overlapping
    secrets.sort_by_key(|s| s.start);
    let mut filtered = Vec::new();
    let mut last_end = 0;
    for secret in secrets {
        if secret.start >= last_end {
            last_end = secret.end;
            filtered.push(secret);
        }
    }
    filtered
}

/// Redact a line by replacing secret characters with bullet characters.
pub fn redact_line(line: &str) -> String {
    let secrets = detect_secrets(line);
    if secrets.is_empty() {
        return line.to_string();
    }
    let chars: Vec<char> = line.chars().collect();
    let mut result = String::with_capacity(line.len());
    // Convert byte offsets to char offsets
    let secret_ranges: Vec<(usize, usize)> = secrets
        .iter()
        .map(|s| {
            let start = line[..s.start].chars().count();
            let end = line[..s.end].chars().count();
            (start, end)
        })
        .collect();
    for (i, c) in chars.iter().enumerate() {
        if secret_ranges.iter().any(|(s, e)| i >= *s && i < *e) {
            result.push('\u{2022}'); // bullet character
        } else {
            result.push(*c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_aws_access_key() {
        let secrets = detect_secrets("key=AKIAIOSFODNN7EXAMPLE");
        assert!(!secrets.is_empty());
        assert_eq!(secrets[0].kind, SecretKind::AwsAccessKey);
    }

    #[test]
    fn test_detect_github_token() {
        let secrets = detect_secrets("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij123456");
        assert!(!secrets.is_empty());
        assert_eq!(secrets[0].kind, SecretKind::GitHubToken);
    }

    #[test]
    fn test_detect_bearer_token() {
        let secrets = detect_secrets("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9==");
        assert!(!secrets.is_empty());
        assert_eq!(secrets[0].kind, SecretKind::BearerToken);
    }

    #[test]
    fn test_detect_private_key() {
        let secrets = detect_secrets("-----BEGIN RSA PRIVATE KEY-----");
        assert!(!secrets.is_empty());
        assert_eq!(secrets[0].kind, SecretKind::PrivateKey);
    }

    #[test]
    fn test_detect_password_in_url() {
        let secrets = detect_secrets("postgres://user:mysecretpassword@localhost:5432/db");
        assert!(!secrets.is_empty());
        assert_eq!(secrets[0].kind, SecretKind::PasswordInUrl);
    }

    #[test]
    fn test_detect_api_key() {
        let secrets = detect_secrets("api_key = \"sk-1234567890abcdefghijklmnop\"");
        assert!(!secrets.is_empty());
        assert_eq!(secrets[0].kind, SecretKind::ApiKey);
    }

    #[test]
    fn test_no_secrets() {
        let secrets = detect_secrets("Hello, world! This is a normal line.");
        assert!(secrets.is_empty());
    }

    #[test]
    fn test_redact_line() {
        let redacted = redact_line("key=AKIAIOSFODNN7EXAMPLE and more text");
        assert!(redacted.contains('\u{2022}'));
        assert!(redacted.contains(" and more text"));
    }

    #[test]
    fn test_redact_line_no_secrets() {
        let original = "Hello, world!";
        let redacted = redact_line(original);
        assert_eq!(original, redacted);
    }

    #[test]
    fn test_overlapping_secrets() {
        // Multiple patterns should not overlap
        let line = "AKIAIOSFODNN7EXAMPLE token=abcdefghijklmnopqrstuvwx";
        let secrets = detect_secrets(line);
        for i in 1..secrets.len() {
            assert!(secrets[i].start >= secrets[i - 1].end);
        }
    }
}
