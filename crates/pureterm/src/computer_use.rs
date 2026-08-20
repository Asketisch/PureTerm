//! GAP-04:Computer Use 桥——为 MCP 工具再导出 `pureterm-computer-use`,
//! 并暴露供 MCP 处理器使用的截图决策/门控辅助函数。

use base64::Engine;

use crate::feed_client::{FeedChoice, FeedOutcome};

pub use pureterm_computer_use::{capture_screen_png, click, key_press, type_text};

/// 以 `data:image/png;base64,...` URL 形式返回的 PNG 字节。
pub fn screenshot_data_url() -> Result<String, String> {
    let bytes = capture_screen_png()?;
    Ok(png_bytes_to_data_url(&bytes))
}

/// 把原始 PNG 字节编码为 `data:image/png;base64,...` URL。纯函数,可在
/// 各平台单元测试(捕获本身仅限 macOS)。
pub fn png_bytes_to_data_url(bytes: &[u8]) -> String {
    let b64 = Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    format!("data:image/png;base64,{b64}")
}

/// GAP-04 安全门控决策。截图会捕获整个桌面
/// (密码、2FA 验证码、私信)并交给 agent——规范要求
/// user consent ("敏感操作需用户确认（类似 Feed Approval）"). Proceed only on an
/// 显式的允许选择(Once/Always/All/Bypass)。`NoGui` / `Timeout` /
/// `Error` 全部**默认失败关闭**——没有明确同意就没有截图。
/// 纯函数,使决策无需 GUI 往返即可单元测试。
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
