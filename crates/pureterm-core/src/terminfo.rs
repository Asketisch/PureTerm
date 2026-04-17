//! PureTerm terminfo 定义。
//!
//! 提供终端能力和 TERM 环境变量设置。

/// PureTerm 的终端名称
pub const TERM_NAME: &str = "pureterm";

/// 我们实际使用的 TERM 别名（与扩展兼容的 xterm-256color）
pub const TERM_COMPAT: &str = "xterm-256color";

/// 必要时安装 PureTerm terminfo。
/// 如果我们的 terminfo 可用则设置 TERM=pureterm，否则回退到 xterm-256color。
pub fn install_terminfo() {
    // 目前，使用 xterm-256color 以获得最大兼容性
    // PureTerm 支持所有 xterm-256color 功能加上扩展
    if std::env::var("TERM").is_err() {
        std::env::set_var("TERM", TERM_COMPAT);
    }
}

/// 获取 PureTerm 的终端标识字符串用于 DA 响应
pub fn version_string() -> String {
    format!("PureTerm/{}", env!("CARGO_PKG_VERSION"))
}

/// 用于 xterm 兼容性的终端名称
pub fn xterm_name() -> &'static str {
    "PureTerm"
}

/// PureTerm 的 DA2 响应功能集
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
