//! GAP-04: Computer Use bridge — re-exports `pureterm-computer-use` for MCP tools.

use base64::Engine;

pub use pureterm_computer_use::{capture_screen_png, click, key_press, type_text};

/// PNG bytes as a `data:image/png;base64,...` URL.
pub fn screenshot_data_url() -> Result<String, String> {
    let bytes = capture_screen_png()?;
    let b64 = Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    Ok(format!("data:image/png;base64,{b64}"))
}
