//! PureTerm terminfo definition.
//!
//! Provides terminal capabilities and TERM environment variable setup.

/// The terminal name for PureTerm
pub const TERM_NAME: &str = "pureterm";

/// The TERM alias we actually use (xterm-256color compatible with extensions)
pub const TERM_COMPAT: &str = "xterm-256color";

/// Install PureTerm terminfo if needed.
/// Sets TERM=pureterm if our terminfo is available, otherwise falls back to xterm-256color.
pub fn install_terminfo() {
    // For now, use xterm-256color for maximum compatibility
    // PureTerm supports all xterm-256color capabilities plus extensions
    if std::env::var("TERM").is_err() {
        std::env::set_var("TERM", TERM_COMPAT);
    }
}

/// Get PureTerm's terminal identification string for DA responses
pub fn version_string() -> String {
    format!("PureTerm/{}", env!("CARGO_PKG_VERSION"))
}

/// Terminal name for xterm compatibility
pub fn xterm_name() -> &'static str {
    "PureTerm"
}

/// PureTerm's feature set for DA2 response
pub fn feature_attributes() -> &'static str {
    "0;95;PureTerm" // VT520 class, 95 columns of features
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_term_constants() {
        assert_eq!(TERM_NAME, "pureterm");
        assert_eq!(TERM_COMPAT, "xterm-256color");
    }

    #[test]
    fn test_version_string() {
        let v = version_string();
        assert!(v.starts_with("PureTerm/"));
    }

    #[test]
    fn test_xterm_name() {
        assert_eq!(xterm_name(), "PureTerm");
    }

    #[test]
    fn test_feature_attributes() {
        assert_eq!(feature_attributes(), "0;95;PureTerm");
    }
}
