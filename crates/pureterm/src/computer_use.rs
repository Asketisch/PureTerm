//! GAP-04: Computer Use bridge — re-exports `pureterm-computer-use` for MCP tools
//! and exposes the screenshot decision/gate helpers used by the MCP handlers.

use base64::Engine;

use crate::feed_client::{FeedChoice, FeedOutcome};

pub use pureterm_computer_use::{capture_screen_png, click, key_press, type_text};

/// PNG bytes as a `data:image/png;base64,...` URL.
pub fn screenshot_data_url() -> Result<String, String> {
    let bytes = capture_screen_png()?;
    Ok(png_bytes_to_data_url(&bytes))
}

/// Encode raw PNG bytes as a `data:image/png;base64,...` URL. Pure so it can be
/// unit-tested on every platform (the capture itself is macOS-only).
pub fn png_bytes_to_data_url(bytes: &[u8]) -> String {
    let b64 = Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    format!("data:image/png;base64,{b64}")
}

/// GAP-04 security gate decision. A screenshot captures the whole desktop
/// (passwords, 2FA codes, DMs) and hands it to the agent — the spec requires
/// user consent ("敏感操作需用户确认（类似 Feed Approval）"). Proceed only on an
/// explicit allow choice (Once/Always/All/Bypass). `NoGui` / `Timeout` /
/// `Error` all **fail closed** — no affirmative consent means no screenshot.
/// Pure so the decision is unit-testable without the GUI round-trip.
pub fn approval_allows_screenshot(outcome: &FeedOutcome) -> bool {
    matches!(outcome, FeedOutcome::Resolved(choice) if choice.is_allow())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_url_round_trips_known_bytes() {
        let raw = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a];
        let url = png_bytes_to_data_url(&raw);
        assert!(url.starts_with("data:image/png;base64,"), "bad prefix: {url}");
        let b64 = url.strip_prefix("data:image/png;base64,").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("must be valid base64");
        assert_eq!(decoded, raw);
    }

    #[test]
    fn approval_allows_on_every_allow_choice() {
        assert!(approval_allows_screenshot(&FeedOutcome::Resolved(FeedChoice::Once)));
        assert!(approval_allows_screenshot(&FeedOutcome::Resolved(FeedChoice::Always)));
        assert!(approval_allows_screenshot(&FeedOutcome::Resolved(FeedChoice::All)));
        assert!(approval_allows_screenshot(&FeedOutcome::Resolved(FeedChoice::Bypass)));
    }

    #[test]
    fn approval_denies_on_deny_choice() {
        assert!(!approval_allows_screenshot(&FeedOutcome::Resolved(FeedChoice::Deny)));
    }

    #[test]
    fn approval_fails_closed_without_affirmative_consent() {
        assert!(!approval_allows_screenshot(&FeedOutcome::NoGui));
        assert!(!approval_allows_screenshot(&FeedOutcome::Timeout));
        assert!(!approval_allows_screenshot(&FeedOutcome::Error("boom".into())));
    }
}
